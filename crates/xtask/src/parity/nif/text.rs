// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity nif --text`: the text dumps (`ToText`) and the saves
//! (`SaveToFile`) of the data format files, made by the GUI build of xEdit
//! through its script adapter (`crates/xtask/oracle/dataformat.pas`), which
//! covers the formats Sniff does not convert: materials, the LOD settings
//! and tree LOD files, FUZ and DDS headers. It also compares `dfCalcHash`,
//! by which upstream finds elements by name, on every element name of the
//! definitions.
//!
//! Every material and LOD file of the corpus archives is checked, and the
//! first `--sample` NIF and FUZ files of each archive. The files are
//! extracted with the port's archive reader into the scratch folder; the GUI
//! runs in its script mode on a plugin made for the run.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use xedit_assets::asset::{AssetFile, AssetKind};
use xedit_assets::data_format::{Def, df_calc_hash};
use xedit_assets::data_format_material::material_defs;
use xedit_assets::data_format_misc::misc_defs;
use xedit_assets::data_format_nif::ni_object_infos;
use xedit_io::archive::Archive;
use xedit_io::encoding::Encoding;

use super::Options;
use crate::memory::{Budget, GIB};
use crate::parity::gui::{self, GuiRun};
use crate::parity::required_var;

/// An empty Fallout 4 master: the TES4 record with its header.
fn empty_plugin() -> Vec<u8> {
    let mut hedr = b"HEDR".to_vec();
    hedr.extend_from_slice(&12u16.to_le_bytes());
    hedr.extend_from_slice(&1.0f32.to_le_bytes());
    hedr.extend_from_slice(&0u32.to_le_bytes());
    hedr.extend_from_slice(&0x800u32.to_le_bytes());
    let mut bytes = b"TES4".to_vec();
    bytes.extend_from_slice(&(hedr.len() as u32).to_le_bytes());
    // ESM flag.
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&131u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&hedr);
    bytes
}

/// The bytes `TStringList.Text := s; SaveToFile` writes: the lines split at
/// CR, LF or CRLF, each ended with CRLF, in the ANSI code page.
fn string_list_bytes(text: &str) -> Vec<u8> {
    let mut out = String::new();
    let mut line = String::new();
    let mut chars = text.chars().peekable();
    let mut pending = false;
    while let Some(ch) = chars.next() {
        match ch {
            '\r' | '\n' => {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str(&line);
                out.push_str("\r\n");
                line.clear();
                pending = false;
            }
            ch => {
                line.push(ch);
                pending = true;
            }
        }
    }
    if pending {
        out.push_str(&line);
        out.push_str("\r\n");
    }
    Encoding::Mbcs(0).get_bytes(&out)
}

fn collect_names(def: &Def, names: &mut Vec<String>) {
    if !def.name.is_empty() {
        names.push(def.name.clone());
    }
    for member in &def.defs {
        collect_names(member, names);
    }
}

/// The element names of every definition, and names that differ only in
/// case or in a character the hash folds.
fn hash_names() -> Vec<String> {
    let mut names = Vec::new();
    for info in &ni_object_infos().ni_objects {
        collect_names(&info.def, &mut names);
    }
    collect_names(&material_defs().bgsm, &mut names);
    collect_names(&material_defs().bgem, &mut names);
    let misc = misc_defs();
    for def in [
        &misc.lod_settings_tes5,
        &misc.lod_settings_fo3,
        &misc.lod_tree_lst,
        &misc.lod_tree_btt,
        &misc.fuz,
        &misc.dds,
    ] {
        collect_names(def, &mut names);
    }
    for extra in [
        "num blocks",
        "NUM BLOCKS",
        "Num_Blocks",
        "Num\u{7f}Blocks",
        "Vertex #12",
        "Children #0",
        "[0]",
        "..",
        "a",
        "@",
        "`",
        "{",
        "0123456789",
        "~!#$%^&*()_+-=",
    ] {
        names.push(extra.to_owned());
    }
    names.sort();
    names.dedup();
    names
}

/// The files of one archive that the check takes.
fn sample(archive: &Archive, sample: usize) -> Vec<String> {
    let mut names: Vec<String> = archive
        .files()
        .iter()
        .map(|entry| entry.name.to_lowercase().replace('/', "\\"))
        .collect();
    names.sort();
    let mut taken: BTreeMap<&'static str, usize> = BTreeMap::new();
    names
        .into_iter()
        .filter(|name| {
            let Some(kind) = AssetKind::from_path(name) else {
                return false;
            };
            let limited = matches!(kind, AssetKind::Nif | AssetKind::Fuz | AssetKind::Dds);
            let count = taken.entry(kind.name()).or_default();
            *count += 1;
            !limited || *count <= sample
        })
        .collect()
}

struct Case {
    game: &'static str,
    archive: String,
    name: String,
    kind: AssetKind,
    data: Vec<u8>,
}

pub fn run(_root: &Path, tag: &str, options: &Options) -> Result<()> {
    let oracle_dir = PathBuf::from(required_var("XEDIT_ORACLE_DIR")?);
    let exe = oracle_dir.join(gui::exe_name("FO4"));
    let scratch = match std::env::var_os("XEDIT_PARITY_SCRATCH") {
        Some(dir) => PathBuf::from(dir).join(tag),
        None => crate::parity::cache_dir()?.join(tag),
    };
    let input = scratch.join("text-input");
    let _ = fs::remove_dir_all(&input);
    fs::create_dir_all(input.join("files"))?;

    let mut cases = Vec::new();
    for &game in &options.games {
        let Some(data) = std::env::var_os(game.data_var).map(PathBuf::from) else {
            continue;
        };
        let mut archives: Vec<PathBuf> = fs::read_dir(&data)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                (name.ends_with(".bsa") || name.ends_with(".ba2"))
                    && (options.archives.is_empty() || options.archives.iter().any(|part| name.contains(part)))
            })
            .collect();
        archives.sort();
        for path in archives {
            let Ok(archive) = Archive::open(&path) else { continue };
            for name in sample(&archive, options.sample) {
                if options.file.as_ref().is_some_and(|part| !name.contains(part.as_str())) {
                    continue;
                }
                let Some(kind) = AssetKind::from_path(&name) else {
                    continue;
                };
                // The textures of a texture archive wait for phase 5 step 2.
                let Ok(Some(data)) = archive.read(&name) else { continue };
                cases.push(Case {
                    game: game.name,
                    archive: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                    name,
                    kind,
                    data,
                });
            }
        }
    }
    let mut files_txt = String::new();
    for (index, case) in cases.iter().enumerate() {
        let extension = case.name.rsplit('.').next().unwrap_or("nif");
        let path = input.join("files").join(format!("{index}.{extension}"));
        fs::write(&path, &case.data)?;
        files_txt.push_str(&path.display().to_string().replace('/', "\\"));
        files_txt.push_str("\r\n");
    }
    fs::write(input.join("files.txt"), files_txt)?;
    let names = hash_names();
    fs::write(
        input.join("names.txt"),
        Encoding::Mbcs(0).get_bytes(&(names.join("\r\n") + "\r\n")),
    )?;
    let plugin = input.join("xEditRustOracle.esm");
    fs::write(&plugin, empty_plugin())?;

    let script = include_str!("../../../oracle/dataformat.pas")
        .replace(
            "{{WORK}}names.txt",
            &format!("{}\\names.txt", input.display()).replace('/', "\\"),
        )
        .replace(
            "{{WORK}}files.txt",
            &format!("{}\\files.txt", input.display()).replace('/', "\\"),
        );
    let budget = Budget::new(16 * GIB);
    let work = scratch.join("text-work");
    let run = GuiRun {
        exe: &exe,
        mode: "FO4",
        star_plugins_txt: !gui::simple_plugins_txt("FO4"),
        plugins: vec![plugin],
        script,
        build_refs: false,
        work: work.clone(),
        timeout: Duration::from_secs(4 * 3600),
        hang_timeout: Duration::from_secs(600),
        budget: &budget,
        expected_peak: 2 * GIB,
        max_memory: 16 * GIB,
    };
    println!("oracle        {} files through the script adapter", cases.len());
    let result = run.run().context("running the GUI oracle")?;
    let status = result.status.join("\n");
    if status.lines().last() != Some("done") {
        gui::remove_work(&work);
        anyhow::bail!("the oracle script failed: {status}");
    }

    // The hashes.
    let oracle_hashes = fs::read_to_string(result.out.join("hashes.txt"))?;
    let mut hash_differences = 0;
    for (name, line) in names.iter().zip(oracle_hashes.lines()) {
        let port = format!("{:08X}", df_calc_hash(name));
        if port != line.trim() {
            hash_differences += 1;
            if hash_differences <= 10 {
                println!("    hash-different {name:?}: port {port}, oracle {}", line.trim());
            }
        }
    }
    println!(
        "hashes: {} names, {} equal, {hash_differences} different",
        names.len(),
        names.len() - hash_differences
    );

    // The dumps and the saves.
    let mut totals: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    let diff_dir = scratch.join("text-diff");
    let _ = fs::remove_dir_all(&diff_dir);
    for (index, case) in cases.iter().enumerate() {
        let stem = result.out.join(index.to_string());
        let oracle_error = fs::read(stem.with_extension("err")).ok();
        let port = AssetFile::load(case.kind, &case.data).and_then(|mut file| {
            let text = file.to_text()?;
            let saved = file.save()?;
            Ok((text, saved))
        });
        let outcome = match (&port, &oracle_error) {
            (Err(error), Some(message)) => {
                if string_list_bytes(&error.0) == *message {
                    "equal-error"
                } else {
                    "port-failed"
                }
            }
            (Err(_), None) => "port-failed",
            (Ok(_), Some(_)) => "oracle-failed",
            (Ok((text, saved)), None) => {
                let oracle_text = fs::read(stem.with_extension("txt")).unwrap_or_default();
                let oracle_saved = fs::read(stem.with_extension("saved")).unwrap_or_default();
                let text = string_list_bytes(text);
                if text != oracle_text {
                    let dir = diff_dir.join(index.to_string());
                    fs::create_dir_all(&dir)?;
                    fs::write(dir.join("port.txt"), &text)?;
                    fs::write(dir.join("oracle.txt"), &oracle_text)?;
                    fs::write(dir.join("name.txt"), &case.name)?;
                    "text-different"
                } else if *saved != oracle_saved {
                    "save-different"
                } else {
                    "equal"
                }
            }
        };
        *totals.entry((case.kind.name(), outcome)).or_default() += 1;
        if outcome != "equal" && outcome != "equal-error" {
            let detail = match (&port, &oracle_error) {
                (Err(error), message) => format!(
                    "port: {}; oracle: {}",
                    error.0,
                    message
                        .as_ref()
                        .map(|m| String::from_utf8_lossy(m).into_owned())
                        .unwrap_or_default()
                ),
                (Ok(_), Some(message)) => format!("oracle: {}", String::from_utf8_lossy(message)),
                _ => format!("see {}", diff_dir.join(index.to_string()).display()),
            };
            println!(
                "    {outcome} {} {} {}: {}",
                case.game,
                case.archive,
                case.name,
                detail.trim()
            );
        }
    }
    gui::remove_work(&work);
    let summary: Vec<String> = totals
        .iter()
        .map(|((kind, outcome), count)| format!("{kind} {outcome} {count}"))
        .collect();
    println!("total: {}", summary.join(", "));
    Ok(())
}
