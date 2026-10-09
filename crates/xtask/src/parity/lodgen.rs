// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity lodgen`: LOD generation of the port against the GUI
//! oracle's LODGen tool mode.
//!
//! Each case (`CASES`) is a game, the plugins to load, the worldspaces to
//! generate LOD for and the options of the LODGen form. The oracle is the
//! GUI build of the game in its LODGen tool mode (`-lodgen`): it loads the
//! plugins, shows the LODGen form (`TfrmLODGen`, whose options it reads
//! from `<APP>LODGen.ini` next to the executable, `[<APP> LOD Options]`),
//! and generates once the form is answered. The harness answers the module
//! selection, checks the case's worldspaces in the form's list (selecting
//! an item and typing a space, which toggles its check box), presses
//! `Generate`, and waits for the closing line of the generator in the
//! message log (`LOD Generator: finished`). The port runs `xedit lodgen`
//! with the same inputs.
//!
//! Both sides read the same private data folder: the plugins copied and
//! the archives of the game's data folder hard linked (copied where the
//! scratch folder is on another volume), with the game's default ini
//! (`<Game>_Default.ini` of the install, which lists the archives) as the
//! game ini. Each side has its own scripts folder (`-S:`, `--scripts`:
//! `LODGenx64.exe`, `Texconvx64.exe`, `LODGen_flat_lod.nif` and the atlas
//! maps of the release's `Edit Scripts`) and output folder (`-O:`,
//! `--output`). Compared are every file of the output folder (the texture
//! atlases, the object LOD meshes `LODGenx64.exe` writes, the tree LOD
//! files), the export files the generator leaves in the scripts folder
//! (`LODGen.txt`, `LODGenAtlasMap.txt`, `LODGenFlatTextures.txt`, with the
//! run folders replaced by a placeholder) and the generator's messages.
//!
//! The oracle's outputs are cached in `<cache>/<tag>/lodgen-oracle/<case>-<key>`
//! (each file zstd-compressed), keyed by the case and the sizes and dates
//! of the plugins and archives. The run folders are in
//! `<scratch>/<tag>/lodgen/<case>/` and removed when everything compares
//! equal, unless `--keep`.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

use super::gui::{
    self, Window, check_list_items, click_button, main_form_log, toggle_check_list_item, visible_windows,
};
use super::hidden::{HiddenChild, HiddenCommand};
use super::nif::fnv;
use super::{GAMES, Game, cache_dir, required_var};
use crate::memory::{GIB, Limit};
use xedit_io::archive::Archive;

const USAGE: &str = "usage: cargo xtask parity lodgen [--case <name or name part>]... [--game <game>]... \
                     [--oracle-only] [--refresh-oracle] [--keep] [--timeout <minutes>] [--list]";

/// A LOD generation run.
struct Case {
    name: &'static str,
    /// The harness name of the game (`fo4`, `sse`, `fnv`).
    game: &'static str,
    /// The plugins to load, in load order.
    plugins: &'static [&'static str],
    /// The editor IDs of the worldspaces to generate LOD for.
    worldspaces: &'static [&'static str],
    /// The values of `[<APP> LOD Options]` of the settings file.
    settings: &'static [(&'static str, &'static str)],
    /// Loose files added to the data folder of the case (synthetic tree
    /// billboards and LOD models the vanilla games do not have): the path
    /// below the data folder and where the content comes from.
    overlay: &'static [(&'static str, Source)],
}

/// The content of a loose file of an overlay.
enum Source {
    /// A file of the game's archives.
    Archive(&'static str),
    /// A text, written as it is.
    Text(&'static str),
}

const FO4_MASTERS: &[&str] = &[
    "Fallout4.esm",
    "DLCRobot.esm",
    "DLCworkshop01.esm",
    "DLCCoast.esm",
    "DLCworkshop02.esm",
    "DLCworkshop03.esm",
    "DLCNukaWorld.esm",
];
const SSE_MASTERS: &[&str] = &[
    "Skyrim.esm",
    "Update.esm",
    "Dawnguard.esm",
    "HearthFires.esm",
    "Dragonborn.esm",
];
const FO3_MASTERS: &[&str] = &[
    "Fallout3.esm",
    "Anchorage.esm",
    "ThePitt.esm",
    "BrokenSteel.esm",
    "PointLookout.esm",
    "Zeta.esm",
];
const FNV_MASTERS: &[&str] = &[
    "FalloutNV.esm",
    "DeadMoney.esm",
    "HonestHearts.esm",
    "OldWorldBlues.esm",
    "LonesomeRoad.esm",
    "GunRunnersArsenal.esm",
];

/// The cases of the check.
const CASES: &[Case] = &[
    Case {
        name: "fo4-sanctuary",
        game: "fo4",
        plugins: FO4_MASTERS,
        worldspaces: &["SanctuaryHillsWorld"],
        settings: &[],
        overlay: &[],
    },
    Case {
        name: "fo4-vr",
        game: "fo4",
        plugins: FO4_MASTERS,
        worldspaces: &["DLC03VRWorldspace"],
        settings: &[],
        overlay: &[],
    },
    Case {
        name: "fo4-nukaworld",
        game: "fo4",
        plugins: FO4_MASTERS,
        worldspaces: &["NukaWorld"],
        settings: &[],
        overlay: &[],
    },
    Case {
        name: "fo4-farharbor",
        game: "fo4",
        plugins: FO4_MASTERS,
        worldspaces: &["DLC03FarHarbor"],
        settings: &[],
        overlay: &[],
    },
    Case {
        name: "sse-sovngarde",
        game: "sse",
        plugins: SSE_MASTERS,
        worldspaces: &["Sovngarde"],
        settings: &[],
        overlay: &[],
    },
    Case {
        name: "sse-soulcairn",
        game: "sse",
        plugins: SSE_MASTERS,
        worldspaces: &["DLC01SoulCairn"],
        settings: &[],
        overlay: &[],
    },
    Case {
        name: "fnv-strip",
        game: "fnv",
        plugins: FNV_MASTERS,
        worldspaces: &["TheStripWorldNew"],
        settings: &[],
        overlay: &[],
    },
    Case {
        name: "fo3-dcworld05",
        game: "fo3",
        plugins: FO3_MASTERS,
        worldspaces: &["DCWorld05"],
        settings: &[],
        overlay: &[],
    },
    Case {
        name: "tes4-all",
        game: "tes4",
        plugins: &["Oblivion.esm"],
        worldspaces: &[],
        settings: &[],
        overlay: &[],
    },
    Case {
        name: "sse-japhetsfolly",
        game: "sse",
        plugins: SSE_MASTERS,
        worldspaces: &["JaphetsFollyWorld"],
        settings: &[],
        overlay: &[],
    },
    Case {
        name: "fnv-gamorrah",
        game: "fnv",
        plugins: FNV_MASTERS,
        worldspaces: &["GamorrahWorld"],
        settings: &[],
        overlay: &[],
    },
    // The atlas in other formats and sizes: DXT5 and BC5, more atlases than
    // one at 1024 pixels.
    Case {
        name: "fnv-strip-dxt5-bc5",
        game: "fnv",
        plugins: FNV_MASTERS,
        worldspaces: &["TheStripWorldNew"],
        settings: &[
            ("AtlasDiffuseFormat", "202"),
            ("AtlasNormalFormat", "205"),
            ("AtlasWidth", "1024"),
            ("AtlasHeight", "1024"),
        ],
        overlay: &[],
    },
    Case {
        name: "fnv-strip-8888-565",
        game: "fnv",
        plugins: FNV_MASTERS,
        worldspaces: &["TheStripWorldNew"],
        settings: &[("AtlasDiffuseFormat", "88"), ("AtlasNormalFormat", "82")],
        overlay: &[],
    },
    Case {
        name: "fnv-strip-888-bc4",
        game: "fnv",
        plugins: FNV_MASTERS,
        worldspaces: &["TheStripWorldNew"],
        settings: &[("AtlasDiffuseFormat", "87"), ("AtlasNormalFormat", "204")],
        overlay: &[],
    },
    Case {
        name: "fnv-strip-dxt1-chunk",
        game: "fnv",
        plugins: FNV_MASTERS,
        worldspaces: &["TheStripWorldNew"],
        settings: &[
            ("AtlasDiffuseFormat", "200"),
            ("DefaultAlphaThreshold", "100"),
            ("Chunk", "1"),
            ("LODLevel", "4"),
            ("LODX", "-4"),
            ("LODY", "-4"),
        ],
        overlay: &[],
    },
    // The tree LOD of the Fallouts with billboards of the trees (the vanilla
    // games have none), brightened, one with an ini of its own.
    Case {
        name: "fnv-strip-trees",
        game: "fnv",
        plugins: FNV_MASTERS,
        worldspaces: &["TheStripWorldNew"],
        settings: &[("TreesBrightness", "5")],
        overlay: &[
            (
                "textures\\terrain\\lodgen\\falloutnv.esm\\NVDatePalm01_00111DA0.dds",
                Source::Archive("textures\\landscape\\trees\\nvdatepalmlod.dds"),
            ),
            (
                "textures\\terrain\\lodgen\\falloutnv.esm\\NVDatePalm02_00111D9F.dds",
                Source::Archive("textures\\landscape\\trees\\nvdatepalmlod.dds"),
            ),
            (
                "textures\\terrain\\lodgen\\falloutnv.esm\\NVQueenPalm01_00111D9D.dds",
                Source::Archive("textures\\landscape\\trees\\nvqueenpalmlod.dds"),
            ),
            (
                "textures\\terrain\\lodgen\\falloutnv.esm\\NVQueenPalm01_00111D9D.txt",
                Source::Text("[LOD]\r\nWidth=600.5\r\nHeight=0\r\nShiftZ=-12.25\r\nScale=1.5\r\n"),
            ),
            (
                "textures\\terrain\\lodgen\\falloutnv.esm\\NVSagoPalm01_00111DA2.dds",
                Source::Archive("textures\\landscape\\trees\\nvsagopalmlod.dds"),
            ),
            (
                "textures\\terrain\\lodgen\\falloutnv.esm\\treejoshANV_0008D47C.dds",
                Source::Archive("textures\\landscape\\trees\\treejosh_lod.dds"),
            ),
        ],
    },
    // The tree LOD of Skyrim (the billboards in a DXT3 atlas, the list and
    // the blocks, rotated at random) and the trees as 3D objects LOD
    // (a LOD model of a tree, and billboards where it has none).
    Case {
        name: "sse-sovngarde-trees",
        game: "sse",
        plugins: SSE_MASTERS,
        worldspaces: &["Sovngarde"],
        settings: &[("TreesBrightness", "-3")],
        overlay: SSE_BILLBOARDS,
    },
    Case {
        name: "sse-sovngarde-trees3d",
        game: "sse",
        plugins: SSE_MASTERS,
        worldspaces: &["Sovngarde"],
        settings: &[("Trees3D", "1"), ("AtlasTextureSize", "256")],
        overlay: SSE_TREES_3D,
    },
];

/// Billboards of Sovngarde's trees, from tree LOD atlases of the game.
const SSE_BILLBOARDS: &[(&str, Source)] = &[
    (
        "textures\\terrain\\lodgen\\skyrim.esm\\TreePineForest01_0001306D.dds",
        Source::Archive("textures\\terrain\\dlc2solstheimworld\\trees\\dlc2solstheimworldtreelod.dds"),
    ),
    (
        "textures\\terrain\\lodgen\\skyrim.esm\\TreePineForest01_0001306D.txt",
        Source::Text("[LOD]\r\nWidth=512\r\nHeight=900\r\nShiftX=1\r\nShiftY=-2\r\nShiftZ=-20\r\nScale=1.25\r\n"),
    ),
    (
        "textures\\terrain\\lodgen\\skyrim.esm\\TreePineForest02_00018A02.dds",
        Source::Archive("textures\\terrain\\dlc1hunterhqworld\\trees\\dlc1hunterhqworldtreelod.dds"),
    ),
    (
        "textures\\terrain\\lodgen\\skyrim.esm\\DeadShrub01_000A731C.dds",
        Source::Archive("textures\\terrain\\dlc2solstheimworld\\trees\\dlc2solstheimworldtreelod.dds"),
    ),
];

/// The billboards, and a LOD model for the 3D trees of one of them.
const SSE_TREES_3D: &[(&str, Source)] = &[
    (
        "meshes\\landscape\\trees\\treepineforest02_lod_0.nif",
        Source::Archive("meshes\\landscape\\trees\\treepineforest01_lod_flat.nif"),
    ),
    (
        "meshes\\landscape\\trees\\treepineforest02_lod_1.nif",
        Source::Archive("meshes\\landscape\\trees\\treepineforest01_lod_flat.nif"),
    ),
    (
        "textures\\terrain\\lodgen\\skyrim.esm\\TreePineForest01_0001306D.dds",
        Source::Archive("textures\\terrain\\dlc2solstheimworld\\trees\\dlc2solstheimworldtreelod.dds"),
    ),
    (
        "textures\\terrain\\lodgen\\skyrim.esm\\DeadShrub01_000A731C.dds",
        Source::Archive("textures\\terrain\\dlc1hunterhqworld\\trees\\dlc1hunterhqworldtreelod.dds"),
    ),
];

struct Options {
    cases: Vec<String>,
    games: Vec<String>,
    oracle_only: bool,
    refresh_oracle: bool,
    keep: bool,
    timeout: Duration,
    list: bool,
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut options = Options {
        cases: Vec::new(),
        games: Vec::new(),
        oracle_only: false,
        refresh_oracle: false,
        keep: false,
        timeout: Duration::from_secs(120 * 60),
        list: false,
    };
    let mut args = args.iter();
    while let Some(&arg) = args.next() {
        match arg {
            "--case" => options.cases.push(args.next().context(USAGE)?.to_string()),
            "--game" => options.games.push(args.next().context(USAGE)?.to_ascii_lowercase()),
            "--oracle-only" => options.oracle_only = true,
            "--refresh-oracle" => options.refresh_oracle = true,
            "--keep" => options.keep = true,
            "--list" => options.list = true,
            "--timeout" => {
                let minutes: u64 = args.next().context(USAGE)?.parse().context(USAGE)?;
                options.timeout = Duration::from_secs(minutes * 60);
            }
            _ => bail!("unknown argument {arg}\n{USAGE}"),
        }
    }
    Ok(options)
}

/// `wbAppName` of a game: the section of the settings file is
/// `<APP> LOD Options` and the settings file `<APP>LODGen.ini`.
fn app_name(game: &Game) -> &'static str {
    match game.mode {
        "TES5VR" => "TES5VR",
        "FO4VR" => "FO4VR",
        mode => GAMES.iter().find(|g| g.mode == mode).map_or("", |g| g.mode),
    }
}

/// The default ini of the game install (`<Data>\..\<Game>_Default.ini`),
/// which lists the archives the game loads.
fn default_ini(game: &Game, data: &Path) -> Result<PathBuf> {
    let name = match game.mode {
        "FO4" => "Fallout4_Default.ini",
        "SSE" | "TES5" => "Skyrim_Default.ini",
        "FNV" | "FO3" => "Fallout_default.ini",
        "TES4" => "Oblivion_default.ini",
        other => bail!("no default ini known for {other}"),
    };
    let path = data.parent().context("data folder without a parent")?.join(name);
    ensure!(path.is_file(), "{} does not exist", path.display());
    Ok(path)
}

/// The scratch folder moved to the drive of `data` (`M:\x` with data on
/// `E:` is `E:\x`), so that hard links reach the game's files.
fn on_volume_of(scratch: &Path, data: &Path) -> PathBuf {
    let drive = |path: &Path| {
        let text = path.to_string_lossy().to_string();
        (text.len() >= 2 && text.as_bytes()[1] == b':').then(|| text[..2].to_ascii_uppercase())
    };
    match (drive(scratch), drive(data)) {
        (Some(from), Some(to)) if from != to => {
            let text = scratch.to_string_lossy().to_string();
            PathBuf::from(format!("{to}{}", &text[2..]))
        }
        _ => scratch.to_path_buf(),
    }
}

/// The extension of the game's archives.
fn archive_extension(game: &Game) -> &'static str {
    match game.mode {
        "FO4" | "FO4VR" | "FO76" | "SF1" => "ba2",
        _ => "bsa",
    }
}

/// The private data folder of a case: the plugins copied, the archives
/// hard linked (or copied across volumes), the default ini next to it.
fn prepare_data(case: &Case, game: &Game, data: &Path, root: &Path) -> Result<PathBuf> {
    let shared = root.join("Data");
    fs::create_dir_all(&shared)?;
    for plugin in case.plugins {
        let target = shared.join(plugin);
        if !target.exists() {
            fs::copy(data.join(plugin), &target).with_context(|| format!("copying {plugin}"))?;
        }
    }
    // A case with loose files has a data folder of its own, the plugins
    // linked to the shared copies.
    let private = if case.overlay.is_empty() {
        shared.clone()
    } else {
        let private = root.join(format!("Data-{}", case.name));
        if private.exists() {
            fs::remove_dir_all(&private)?;
        }
        fs::create_dir_all(&private)?;
        for plugin in case.plugins {
            let target = private.join(plugin);
            if fs::hard_link(shared.join(plugin), &target).is_err() {
                fs::copy(shared.join(plugin), &target)?;
            }
        }
        private
    };
    let extension = archive_extension(game);
    for entry in fs::read_dir(data)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case(extension))
        {
            continue;
        }
        let target = private.join(entry.file_name());
        if target.exists() {
            continue;
        }
        if fs::hard_link(&path, &target).is_err() {
            fs::copy(&path, &target).with_context(|| format!("copying {}", path.display()))?;
        }
    }
    let ini = root.join("game.ini");
    if !ini.exists() {
        fs::copy(default_ini(game, data)?, &ini)?;
    }
    for (name, source) in case.overlay {
        let bytes = match source {
            Source::Text(text) => text.as_bytes().to_vec(),
            Source::Archive(file) => read_from_archives(&private, extension, file)?,
        };
        let target = private.join(name.replace('\\', "/"));
        fs::create_dir_all(target.parent().context("overlay file without a folder")?)?;
        fs::write(&target, bytes)?;
    }
    Ok(private)
}

/// A file of the archives of a data folder.
fn read_from_archives(data: &Path, extension: &str, file: &str) -> Result<Vec<u8>> {
    for entry in fs::read_dir(data)? {
        let path = entry?.path();
        if !path
            .extension()
            .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case(extension))
        {
            continue;
        }
        let archive = Archive::open(&path).with_context(|| format!("opening {}", path.display()))?;
        if let Some(bytes) = archive.read(file).with_context(|| format!("reading {file}"))? {
            return Ok(bytes);
        }
    }
    bail!("no archive of {} has {file}", data.display())
}

/// The scripts folder of a side: the LODGen tools and the atlas maps of
/// the release's `Edit Scripts`.
fn prepare_scripts(oracle_dir: &Path, scripts: &Path) -> Result<()> {
    fs::create_dir_all(scripts)?;
    let source = oracle_dir.join("Edit Scripts");
    for entry in fs::read_dir(&source).with_context(|| format!("reading {}", source.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".exe") || lower.ends_with(".nif") || lower.contains("-atlasmap-") {
            fs::copy(entry.path(), scripts.join(&name))?;
        }
    }
    Ok(())
}

/// The settings file of the LODGen form.
fn settings_text(case: &Case, game: &Game) -> String {
    let mut text = format!("[{} LOD Options]\r\n", app_name(game));
    for (name, value) in case.settings {
        text.push_str(&format!("{name}={value}\r\n"));
    }
    text
}

/// The key of the oracle's cached outputs: the case and the inputs as
/// their sizes and dates.
fn oracle_key(case: &Case, game: &Game, data: &Path) -> Result<String> {
    let mut text = format!(
        "{}|{:?}|{:?}|{}",
        case.name,
        case.plugins,
        case.worldspaces,
        settings_text(case, game)
    );
    let extension = archive_extension(game);
    for (name, source) in case.overlay {
        match source {
            Source::Archive(file) => text.push_str(&format!("|{name}<{file}")),
            Source::Text(content) => text.push_str(&format!("|{name}={content}")),
        }
    }
    let mut names: Vec<PathBuf> = case.plugins.iter().map(|plugin| data.join(plugin)).collect();
    for entry in fs::read_dir(data)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case(extension))
        {
            names.push(path);
        }
    }
    names.sort();
    for path in names {
        let meta = fs::metadata(&path).with_context(|| format!("reading {}", path.display()))?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |time| time.as_secs());
        text.push_str(&format!("|{}:{}:{}", path.display(), meta.len(), modified));
    }
    Ok(format!("{}-{:016x}", case.name, fnv(text.as_bytes())))
}

/// The files the generator writes into the scripts folder.
const EXPORT_FILES: &[&str] = &["LODGen.txt", "LODGenAtlasMap.txt", "LODGenFlatTextures.txt"];

/// The outputs of one side: the relative path of each file and its bytes.
type Outputs = BTreeMap<String, Vec<u8>>;

/// The outputs of a finished side: the output folder (`out\...`), the
/// export files (`scripts\...`, with the run folder replaced by
/// `{WORK}`) and the messages (`log.txt`).
fn collect(side: &Path, work_text: &str, root_text: &str, log: &str) -> Result<Outputs> {
    let normalize = |bytes: &[u8]| normalize_paths(&normalize_paths(bytes, work_text, "{WORK}"), root_text, "{ROOT}");
    let mut outputs = Outputs::new();
    let out = side.join("out");
    if out.is_dir() {
        for entry in walk(&out)? {
            let relative = entry
                .strip_prefix(&out)?
                .to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase();
            outputs.insert(format!("out\\{relative}"), fs::read(&entry)?);
        }
    }
    for name in EXPORT_FILES {
        let path = side.join("scripts").join(name);
        if path.is_file() {
            let bytes = fs::read(&path)?;
            outputs.insert(format!("scripts\\{name}"), normalize(&bytes));
        }
    }
    outputs.insert("log.txt".to_owned(), normalize(log.as_bytes()));
    Ok(outputs)
}

/// Every file below a folder.
fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// The text with the run folder of a side replaced by `{WORK}`, compared
/// without case.
fn normalize_paths(bytes: &[u8], work_text: &str, placeholder: &str) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let lower = text.to_ascii_lowercase();
    let needle = work_text.to_ascii_lowercase();
    if needle.is_empty() {
        return bytes.to_vec();
    }
    let mut result = String::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find(&needle) {
        result.push_str(&text[at..at + found]);
        result.push_str(placeholder);
        at += found + needle.len();
    }
    result.push_str(&text[at..]);
    result.into_bytes()
}

/// Writes the outputs of the oracle into the cache.
fn store(outputs: &Outputs, dir: &Path) -> Result<()> {
    let partial = dir.with_extension("partial");
    if partial.exists() {
        fs::remove_dir_all(&partial)?;
    }
    for (name, bytes) in outputs {
        let path = partial.join(format!("{}.zst", name.replace('\\', "/")));
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, zstd::encode_all(&bytes[..], 9)?)?;
    }
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::rename(&partial, dir)?;
    Ok(())
}

/// Reads the cached outputs of the oracle.
fn load(dir: &Path) -> Result<Outputs> {
    let mut outputs = Outputs::new();
    for path in walk(dir)? {
        let relative = path.strip_prefix(dir)?.to_string_lossy().replace('/', "\\");
        let name = relative.strip_suffix(".zst").unwrap_or(&relative).to_owned();
        outputs.insert(name, zstd::decode_all(&fs::read(&path)?[..])?);
    }
    Ok(outputs)
}

/// Runs the GUI oracle on a case; returns its outputs.
fn run_oracle(
    case: &Case,
    game: &Game,
    oracle_dir: &Path,
    root: &Path,
    data: &Path,
    timeout: Duration,
) -> Result<Outputs> {
    let side = root.join("oracle");
    if side.exists() {
        fs::remove_dir_all(&side)?;
    }
    let bin = side.join("bin");
    let scripts = side.join("scripts");
    let out = side.join("out");
    let my_games = side.join("mygames");
    for dir in [&bin, &out, &my_games.join("Saves")] {
        fs::create_dir_all(dir)?;
    }
    prepare_scripts(oracle_dir, &scripts)?;
    let exe_name = gui::exe_name(game.mode);
    let exe = bin.join(exe_name);
    fs::copy(oracle_dir.join(exe_name), &exe)?;
    fs::write(
        bin.join(format!("{}LODGen.ini", app_name(game))),
        settings_text(case, game),
    )?;
    let mut plugins_txt = String::new();
    for plugin in case.plugins {
        if !gui::simple_plugins_txt(game.mode) {
            plugins_txt.push('*');
        }
        plugins_txt.push_str(plugin);
        plugins_txt.push_str("\r\n");
    }
    let plugins = side.join("plugins.txt");
    fs::write(&plugins, plugins_txt)?;
    let work_text = format!("{}\\", gui::windows_path(&side));
    let ini = root.parent().context("case folder without a parent")?.join("game.ini");

    let mut command = HiddenCommand::new(&exe);
    command
        .arg(format!("-{}", game.mode))
        .arg("-lodgen")
        .arg(format!("-D:{}\\", gui::windows_path(data)))
        .arg(format!("-P:{}", gui::windows_path(&plugins)))
        .arg(format!("-T:{work_text}temp\\"))
        .arg(format!("-C:{work_text}cache\\"))
        .arg(format!("-B:{work_text}backup\\"))
        .arg(format!("-S:{work_text}scripts\\"))
        .arg(format!("-O:{work_text}out\\"))
        .arg(format!("-M:{}\\", gui::windows_path(&my_games)))
        .arg(format!("-I:{}", gui::windows_path(&ini)))
        .arg(format!("-G:{}\\", gui::windows_path(&my_games.join("Saves"))))
        .arg("-IKnowWhatImDoing")
        .current_dir(&side);
    let mut child = command.spawn().with_context(|| format!("starting {}", exe.display()))?;
    let limit = match Limit::apply(&child, 24 * GIB) {
        Ok(limit) => limit,
        Err(error) => {
            child.kill();
            return Err(error);
        }
    };
    let watched = watch(&mut child, &limit, case, timeout);
    child.kill();
    let log = watched?;
    let root_text = format!(
        "{}\\",
        gui::windows_path(root.parent().context("case folder without a parent")?)
    );
    collect(&side, &work_text, &root_text, &log)
}

/// Answers the module selection and the LODGen form, and waits for the
/// generator's closing line; returns the message log.
fn watch(child: &mut HiddenChild, limit: &Limit, case: &Case, timeout: Duration) -> Result<String> {
    let start = Instant::now();
    let hang_timeout = Duration::from_secs(10 * 60);
    let mut answered: Vec<isize> = Vec::new();
    let mut dialogs: Vec<(isize, Instant)> = Vec::new();
    let mut last_cpu = (0u64, Instant::now());
    let mut last_trace = Instant::now();
    loop {
        if let Some(code) = child.try_wait()? {
            bail!("the oracle exited (exit code: {code}) before the generator finished");
        }
        ensure!(
            start.elapsed() < timeout,
            "the oracle did not finish within {} minutes",
            timeout.as_secs() / 60
        );
        let cpu = limit.cpu_time();
        if cpu != last_cpu.0 {
            last_cpu = (cpu, Instant::now());
        } else if last_cpu.1.elapsed() > hang_timeout {
            bail!(
                "the oracle used no CPU time for {} seconds (hang)",
                hang_timeout.as_secs()
            );
        }
        let mut waiting = false;
        let windows = visible_windows(child.id());
        // `XEDIT_LODGEN_TRACE` prints the windows of the oracle and the end
        // of its log every 15 seconds.
        if std::env::var_os("XEDIT_LODGEN_TRACE").is_some() && last_trace.elapsed() > Duration::from_secs(15) {
            last_trace = Instant::now();
            for window in &windows {
                let log = if window.class == "TfrmMain" {
                    gui::tail(&main_form_log(window.handle), 8)
                } else {
                    String::new()
                };
                eprintln!(
                    "[{} s] {} \"{}\" visible={} {:?} {}",
                    start.elapsed().as_secs(),
                    window.class,
                    window.title,
                    window.visible,
                    window.texts,
                    log
                );
            }
        }
        for window in windows {
            match window.class.as_str() {
                "TfrmMain" => {
                    let log = main_form_log(window.handle);
                    if let Some(line) = log.lines().find(|line| line.starts_with("Fatal:")) {
                        bail!("the oracle stopped: {line}");
                    }
                    if log.contains("LOD Generator: finished") {
                        return Ok(log);
                    }
                }
                // The system's windows for the input of the process (the
                // keyboard indicator, the input method).
                "TApplication" | "UAC_InputIndicatorOverlayWnd" | "IME" | "MSCTFIME UI" => {}
                "TfrmModuleSelect" => {
                    if !answered.contains(&window.handle) {
                        if window.visible {
                            ensure!(click_button(&window, "OK"), "the module selection has no OK button");
                            answered.push(window.handle);
                        } else {
                            waiting = true;
                        }
                    }
                }
                "TfrmLODGen" => {
                    if !answered.contains(&window.handle) {
                        if window.visible {
                            answer_lodgen_form(&window, case)?;
                            answered.push(window.handle);
                        } else {
                            waiting = true;
                        }
                    }
                }
                // A window without a title or text is no prompt (the
                // system's input indicator of the process shows as one).
                _ if window.title.is_empty() && window.texts.is_empty() => {}
                _ => {
                    let seen = match dialogs.iter().find(|(handle, _)| *handle == window.handle) {
                        Some((_, seen)) => *seen,
                        None => {
                            dialogs.push((window.handle, Instant::now()));
                            Instant::now()
                        }
                    };
                    if seen.elapsed() > Duration::from_secs(10) {
                        bail!(
                            "the oracle shows a dialog [{}] \"{}\": {}",
                            window.class,
                            window.title,
                            window.texts.join(" | ")
                        );
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(if waiting { 10 } else { 250 }));
    }
}

/// Checks exactly the case's worldspaces in the form's list and presses
/// `Generate`. The list names a worldspace `<EditorID> "<name>" [WRLD:...]`;
/// the form checks the default worldspace (`0000003C`, or `000DA726` in New
/// Vegas) at the top and a single one when there is no other.
fn answer_lodgen_form(window: &Window, case: &Case) -> Result<()> {
    // The form fills the list before it shows; give it a moment to paint.
    std::thread::sleep(Duration::from_millis(500));
    let (list, items) = check_list_items(window).context("the LODGen form has no worldspace list")?;
    let default_checked = |index: usize, item: &str| {
        (index == 0 && (item.contains("[WRLD:0000003C]") || item.contains("[WRLD:000DA726]"))) || items.len() == 1
    };
    let mut found = 0;
    for (index, item) in items.iter().enumerate() {
        let editor_id = item.split(' ').next().unwrap_or_default();
        let wanted = case.worldspaces.iter().any(|ws| ws.eq_ignore_ascii_case(editor_id));
        if wanted {
            found += 1;
        }
        if wanted != default_checked(index, item) {
            toggle_check_list_item(list, index);
        }
    }
    ensure!(
        found == case.worldspaces.len(),
        "the LODGen form lists {} of the worldspaces {:?}: {:?}",
        found,
        case.worldspaces,
        items
    );
    std::thread::sleep(Duration::from_millis(200));
    ensure!(
        click_button(window, "Generate"),
        "the LODGen form has no Generate button"
    );
    Ok(())
}

pub fn run(root_dir: &Path, tag: &str, args: &[&str]) -> Result<()> {
    let options = parse(args)?;
    if options.list {
        for case in CASES {
            println!("{:<24} {:<5} {}", case.name, case.game, case.worldspaces.join(", "));
        }
        return Ok(());
    }
    let oracle_dir = PathBuf::from(required_var("XEDIT_ORACLE_DIR")?);
    let cache = cache_dir()?.join(tag).join("lodgen-oracle");
    let scratch = match std::env::var_os("XEDIT_PARITY_SCRATCH") {
        Some(dir) => PathBuf::from(dir).join(tag),
        None => cache_dir()?.join(tag),
    };
    let port_exe = if options.oracle_only {
        PathBuf::new()
    } else {
        // A copy of the port, as other runs may build it meanwhile.
        let built = super::build_port(root_dir)?;
        let copy = scratch.join("xedit-lodgen.exe");
        fs::create_dir_all(&scratch)?;
        fs::copy(&built, &copy)?;
        copy
    };
    let mut failures = 0;
    for case in CASES {
        let selects = |part: &String| {
            case.name == part.as_str()
                || (!CASES.iter().any(|other| other.name == part.as_str()) && case.name.contains(part.as_str()))
        };
        if !options.cases.is_empty() && !options.cases.iter().any(selects) {
            continue;
        }
        if !options.games.is_empty() && !options.games.iter().any(|game| game == case.game) {
            continue;
        }
        let game = GAMES
            .iter()
            .find(|game| game.name == case.game)
            .context("unknown game")?;
        let Some(data) = std::env::var_os(game.data_var).map(PathBuf::from) else {
            println!("skipped       {}: {} is not set", case.name, game.data_var);
            continue;
        };
        // One data folder per game, on the volume of the game's data so
        // that its archives can be hard linked.
        let game_root = on_volume_of(&scratch, &data).join("lodgen").join(game.name);
        fs::create_dir_all(&game_root)?;
        let private = prepare_data(case, game, &data, &game_root)?;
        let root = game_root.join(case.name);
        fs::create_dir_all(&root)?;
        let key = oracle_key(case, game, &data)?;
        let cached = cache.join(&key);
        let started = Instant::now();
        let oracle = if cached.is_dir() && !options.refresh_oracle {
            load(&cached)?
        } else {
            match run_oracle(case, game, &oracle_dir, &root, &private, options.timeout) {
                Ok(outputs) => {
                    fs::create_dir_all(&cache)?;
                    store(&outputs, &cached)?;
                    outputs
                }
                Err(error) => {
                    println!("oracle-failed {}: {error:#}", case.name);
                    failures += 1;
                    continue;
                }
            }
        };
        let files = oracle.keys().filter(|name| name.starts_with("out\\")).count();
        let bytes: usize = oracle.values().map(Vec::len).sum();
        println!(
            "oracle        {}: {} output files, {} bytes ({} s)",
            case.name,
            files,
            bytes,
            started.elapsed().as_secs()
        );
        if options.oracle_only {
            if !options.keep {
                gui::remove_work(&root.join("oracle"));
            }
            continue;
        }
        let started = Instant::now();
        // The tree rotations are random from a clock seed: the port runs
        // from the seed the oracle's rotations show.
        let seed = oracle_seed(&oracle);
        if seed.is_none() && !tree_rotations(&oracle).iter().all(Vec::is_empty) {
            println!(
                "note          {}: no seed explains the oracle's tree rotations",
                case.name
            );
        }
        let port = match run_port(case, game, &port_exe, &oracle_dir, &root, &private, seed) {
            Ok(outputs) => outputs,
            Err(error) => {
                println!("port-failed   {}: {error:#}", case.name);
                failures += 1;
                continue;
            }
        };
        let differences = compare(&oracle, &port, seed);
        let equal = differences.is_empty();
        println!(
            "{:<13} {}: {} files, {} log lines ({} s){}",
            if equal { "equal" } else { "different" },
            case.name,
            oracle.len(),
            log_lines(oracle.get("log.txt").map_or(&[][..], Vec::as_slice)).len(),
            started.elapsed().as_secs(),
            if equal {
                String::new()
            } else {
                format!("\n  {}", differences.join("\n  "))
            }
        );
        if equal {
            if !options.keep {
                gui::remove_work(&root.join("oracle"));
                gui::remove_work(&root.join("port"));
            }
        } else {
            failures += 1;
            // the oracle's outputs next to the port's, to compare by hand
            let kept = root.join("oracle-outputs");
            if kept.exists() {
                fs::remove_dir_all(&kept)?;
            }
            for (name, bytes) in &oracle {
                let path = kept.join(name.replace('\\', "/"));
                fs::create_dir_all(path.parent().unwrap())?;
                fs::write(path, bytes)?;
            }
        }
    }
    ensure!(failures == 0, "{failures} cases failed");
    Ok(())
}

/// The lines of a generator's log that are xEdit's: after the loader,
/// without the encodings of the string files, the archive lines and the
/// output of `LODGenx64.exe` (from the `Running` line to the end of the
/// objects LOD), trimmed.
fn log_lines(log: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(log);
    let mut lines = Vec::new();
    let start = text.find("Background Loader: finished").unwrap_or(0);
    let mut in_tool = false;
    for line in text[start..].lines().skip(usize::from(
        start > 0 || text.starts_with("Background Loader: finished"),
    )) {
        // The memo's text may end in NULs.
        let line = line.trim_matches(|c: char| c.is_whitespace() || c == '\0');
        if line.contains("] Using encoding")
            || line.ends_with("] Loading Resources.")
            || line.ends_with("] Setting Resource Path.")
            || line.contains("] Version: ")
            || line.starts_with("Warning: <Can't find ")
        {
            continue;
        }
        if in_tool {
            if line.ends_with("] Objects LOD Done.") || line.contains("Objects LOD generation error") {
                in_tool = false;
            } else {
                continue;
            }
        }
        // the elapsed time of the Oblivion generator
        let line = match line.split_once("] ") {
            Some((time, rest))
                if time.starts_with('[') && time[1..].chars().all(|c| c.is_ascii_digit() || c == ':') =>
            {
                format!("[time] {rest}")
            }
            _ => line.to_owned(),
        };
        if line.contains("] Running \"") {
            in_tool = true;
        }
        lines.push(line);
    }
    lines
}

/// The differences of two runs: the files only one has or whose bytes
/// differ, and the first line of the logs that differs.
fn compare(oracle: &Outputs, port: &Outputs, seed: Option<u32>) -> Vec<String> {
    let mut differences = Vec::new();
    // The rotations of the tree references, oracle and port, of the block
    // files equal but for them.
    let mut rotations = Vec::new();
    let mut shifted = false;
    for (name, bytes) in oracle {
        if name == "log.txt" {
            continue;
        }
        let other = port.get(name);
        if is_tree_block(name)
            && let (Some(other), Some(_)) = (other, seed)
            && other.len() == bytes.len()
        {
            let offsets = rotation_offsets(bytes);
            let mut masked = (bytes.clone(), other.clone());
            for &at in &offsets {
                masked.0[at..at + 4].fill(0);
                masked.1[at..at + 4].fill(0);
            }
            if masked.0 == masked.1 {
                shifted |= bytes != other;
                rotations.extend(
                    offsets
                        .iter()
                        .map(|&at| (rotation_at(bytes, at), rotation_at(other, at))),
                );
                continue;
            }
        }
        match other {
            None => differences.push(format!("only the oracle wrote {name}")),
            Some(other) if other != bytes => {
                let at = bytes
                    .iter()
                    .zip(other)
                    .position(|(a, b)| a != b)
                    .unwrap_or(bytes.len().min(other.len()));
                differences.push(format!(
                    "{name}: differs at byte {at} (oracle {} bytes, port {} bytes)",
                    bytes.len(),
                    other.len()
                ));
            }
            _ => {}
        }
    }
    for name in port.keys() {
        if name != "log.txt" && !oracle.contains_key(name) {
            differences.push(format!("only the port wrote {name}"));
        }
    }
    if shifted && let Some(seed) = seed {
        if same_draw_order(&rotations, seed) {
            let moved = rotations.iter().filter(|(a, b)| a.to_bits() != b.to_bits()).count();
            println!(
                "note          {moved} of {} tree rotations come later in the oracle's Random sequence (numbers the GUI drew elsewhere)",
                rotations.len()
            );
        } else {
            differences.push("tree rotations: not of the same Random draws as the oracle's".to_owned());
        }
    }
    let (a, b) = (
        log_lines(oracle.get("log.txt").map_or(&[][..], Vec::as_slice)),
        log_lines(port.get("log.txt").map_or(&[][..], Vec::as_slice)),
    );
    if a != b {
        let at = a
            .iter()
            .zip(&b)
            .position(|(x, y)| x != y)
            .unwrap_or(a.len().min(b.len()));
        differences.push(format!(
            "log line {at}: oracle {:?}, port {:?} ({} and {} lines)",
            a.get(at),
            b.get(at),
            a.len(),
            b.len()
        ));
    }
    differences
}

/// Runs the port's `xedit lodgen` on a case; returns its outputs as the
/// oracle's are collected.
/// Whether an output is a block file of tree references.
fn is_tree_block(name: &str) -> bool {
    name.ends_with(".btt") || name.ends_with(".dtl")
}

/// The offsets of the rotations of the references of a block file
/// (`TwbLodTES5TreeBlock`: the type count, then per type its index, its
/// reference count and 32 bytes per reference, the rotation at 12).
fn rotation_offsets(bytes: &[u8]) -> Vec<usize> {
    let int = |at: usize| bytes.get(at..at + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()));
    let mut offsets = Vec::new();
    let mut p = 4;
    for _ in 0..int(0).unwrap_or(0).max(0) {
        let Some(count) = int(p + 4) else { break };
        p += 8;
        for _ in 0..count.max(0) {
            if p + 16 > bytes.len() {
                break;
            }
            offsets.push(p + 12);
            p += 32;
        }
    }
    offsets
}

fn rotation_at(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// The rotations of the tree references of the oracle's block files
/// (`.btt`, `.dtl`), per file.
fn tree_rotations(oracle: &Outputs) -> Vec<Vec<f32>> {
    oracle
        .iter()
        .filter(|(name, _)| is_tree_block(name))
        .map(|(_, bytes)| {
            rotation_offsets(bytes)
                .into_iter()
                .map(|at| rotation_at(bytes, at))
                .collect()
        })
        .collect()
}

const RAND_MULTIPLIER: u32 = 0x0808_8405;

/// The rotation `2 * Pi * Random` of the draw that leaves `RandSeed` at
/// `seed`.
fn rotation_of(seed: u32) -> f32 {
    (2.0 * std::f64::consts::PI * (f64::from(seed) / 4_294_967_296.0)) as f32
}

/// The index of each rotation in the `Random` sequence from `seed`, within
/// `draws` draws (the first draw is index 0).
fn draw_indices(seed: u32, draws: usize) -> HashMap<u32, usize> {
    let mut indices = HashMap::new();
    let mut state = seed;
    for index in 0..draws {
        state = state.wrapping_mul(RAND_MULTIPLIER).wrapping_add(1);
        indices.entry(rotation_of(state).to_bits()).or_insert(index);
    }
    indices
}

/// Whether the rotations of the references differ only by numbers the
/// oracle drew elsewhere: the GUI draws `Random` outside the generator now
/// and then (the same case shows it in some runs only), which shifts the
/// rotations of the references after it. The port's draws must come in
/// the same order and the oracle's no earlier.
fn same_draw_order(pairs: &[(f32, f32)], seed: u32) -> bool {
    let indices = draw_indices(seed, pairs.len() * 4 + 64);
    let mut drawn = Vec::new();
    for (oracle, port) in pairs {
        match (indices.get(&oracle.to_bits()), indices.get(&port.to_bits())) {
            (Some(&oracle), Some(&port)) if oracle >= port => drawn.push((port, oracle)),
            _ => return false,
        }
    }
    drawn.sort_unstable();
    drawn
        .windows(2)
        .all(|pair| pair[0].0 < pair[1].0 && pair[0].1 < pair[1].1)
}

/// The `RandSeed` the oracle started from. Upstream seeds `Random` from
/// the clock (`Randomize` at startup) and draws one number per tree
/// reference for its rotation (`2 * Pi * Random`); the seed is the one
/// whose sequence holds every rotation of the oracle's block files within
/// a few draws per reference. None without tree references.
fn oracle_seed(oracle: &Outputs) -> Option<u32> {
    const MULTIPLIER: u32 = RAND_MULTIPLIER;
    let rotation = rotation_of;
    let files = tree_rotations(oracle);
    let all: Vec<u32> = files.iter().flatten().map(|r| r.to_bits()).collect();
    if all.is_empty() {
        return None;
    }
    // The inverse of the multiplier modulo 2^32 (Newton's iteration).
    let mut inverse: u32 = MULTIPLIER;
    for _ in 0..5 {
        inverse = inverse.wrapping_mul(2u32.wrapping_sub(MULTIPLIER.wrapping_mul(inverse)));
    }
    // The first draw is the first reference of one of the files.
    for first in files.iter().filter_map(|rotations| rotations.first()) {
        let guess = (f64::from(*first) / (2.0 * std::f64::consts::PI) * 4_294_967_296.0) as i64;
        for delta in -4096..=4096i64 {
            let candidate = (guess + delta).rem_euclid(1 << 32) as u32;
            if rotation(candidate).to_bits() != first.to_bits() {
                continue;
            }
            let mut drawn = std::collections::HashSet::new();
            let mut seed = candidate;
            drawn.insert(rotation(seed).to_bits());
            // A reference the block has already draws too.
            for _ in 1..all.len() * 4 + 64 {
                seed = seed.wrapping_mul(MULTIPLIER).wrapping_add(1);
                drawn.insert(rotation(seed).to_bits());
            }
            if all.iter().all(|bits| drawn.contains(bits)) {
                return Some(candidate.wrapping_sub(1).wrapping_mul(inverse));
            }
        }
    }
    None
}

fn run_port(
    case: &Case,
    game: &Game,
    port_exe: &Path,
    oracle_dir: &Path,
    root: &Path,
    data: &Path,
    seed: Option<u32>,
) -> Result<Outputs> {
    let side = root.join("port");
    if side.exists() {
        fs::remove_dir_all(&side)?;
    }
    let bin = side.join("bin");
    let scripts = side.join("scripts");
    let out = side.join("out");
    for dir in [&bin, &out] {
        fs::create_dir_all(dir)?;
    }
    prepare_scripts(oracle_dir, &scripts)?;
    let settings = bin.join(format!("{}LODGen.ini", app_name(game)));
    fs::write(&settings, settings_text(case, game))?;
    let work_text = format!("{}\\", gui::windows_path(&side));
    let game_root = root.parent().context("case folder without a parent")?;
    let root_text = format!("{}\\", gui::windows_path(game_root));
    let ini = game_root.join("game.ini");
    let mut command = std::process::Command::new(port_exe);
    command.arg("--json").arg("--edit").arg("--game").arg(game.name);
    // The games whose plugin list is only of the active plugins load them in
    // the order of their dates, as the GUI does.
    let mut plugins: Vec<&str> = case.plugins.to_vec();
    if gui::simple_plugins_txt(game.mode) {
        plugins.sort_by_key(|plugin| fs::metadata(data.join(plugin)).and_then(|meta| meta.modified()).ok());
    }
    for plugin in plugins {
        command
            .arg("--load")
            .arg(format!("{}\\{plugin}", gui::windows_path(data)));
    }
    command
        .arg("lodgen")
        .arg("--settings")
        .arg(gui::windows_path(&settings))
        .arg("--output")
        .arg(format!("{work_text}out\\"))
        .arg("--scripts")
        .arg(format!("{work_text}scripts\\"))
        .arg("--temp")
        .arg(format!("{work_text}temp\\"))
        .arg("--data")
        .arg(format!("{}\\", gui::windows_path(data)))
        .arg("--game-ini")
        .arg(gui::windows_path(&ini));
    for worldspace in case.worldspaces {
        command.arg("--worldspace").arg(worldspace);
    }
    if let Some(seed) = seed {
        command.arg("--seed").arg(seed.to_string());
    }
    // A plain working folder: `LODGenx64.exe` (.NET) fails on a verbatim
    // (`\\?\`) one, which the harness's may be.
    let output = command
        .current_dir(&side)
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("starting {}", port_exe.display()))?;
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "the port printed no JSON: {}",
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .last()
                .unwrap_or_default()
        )
    })?;
    if response["ok"] != serde_json::Value::Bool(true) {
        bail!("the port failed: {}", response["error"]);
    }
    let mut log = String::from("Background Loader: finished\r\n");
    for line in response["result"]["messages"].as_array().into_iter().flatten() {
        log.push_str(line.as_str().unwrap_or_default());
        log.push_str("\r\n");
    }
    fs::write(side.join("log.txt"), &log)?;
    collect(&side, &work_text, &root_text, &log)
}
