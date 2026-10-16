// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The archives a session loads (`TLoaderThread.Execute`, issue #32): the
//! archives of the game ini first, then those of each plugin by partial
//! name, then the data folder; the string tables of a localized plugin read
//! from an archive that is neither `<plugin>.bsa` nor its interface or
//! localization archive; and `--skip-bsa`, which leaves the data folder alone.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use xedit_core::interface::globals::test_lock;
use xedit_io::archive::{ArchiveType, PackFile};
use xedit_io::temp::TempDir;
use xedit_session::resources::{ResourceOptions, set_options};
use xedit_session::{Registry, Session};

fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn main_record(signature: &[u8; 4], flags: u32, form_id: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&form_id.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&44u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

/// `Loc.esm`, localized, with one string game setting of string ID 1.
fn plugin() -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&1u32.to_le_bytes());
    hedr.extend_from_slice(&0x801u32.to_le_bytes());
    let mut data = sub_record(b"HEDR", &hedr);
    data.extend(sub_record(b"CNAM", b"xedit-rust\0"));
    let mut bytes = main_record(b"TES4", 0x81, 0, &data);
    let mut gmst = sub_record(b"EDID", b"sProbe\0");
    gmst.extend(sub_record(b"DATA", &1u32.to_le_bytes()));
    let record = main_record(b"GMST", 0, 0x800, &gmst);
    bytes.extend_from_slice(b"GRUP");
    bytes.extend_from_slice(&((24 + record.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(b"GMST");
    bytes.extend_from_slice(&[0u8; 12]);
    bytes.extend(record);
    bytes
}

/// A `.STRINGS` table with one string, or a length-prefixed one without.
fn table(text: Option<&str>) -> Vec<u8> {
    let mut bytes = Vec::new();
    match text {
        Some(text) => {
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&(text.len() as u32 + 1).to_le_bytes());
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
            bytes.push(0);
        }
        None => bytes.extend_from_slice(&[0u8; 8]),
    }
    bytes
}

/// A Skyrim SE archive of the files.
fn archive(path: &Path, files: &[(&str, Vec<u8>)]) {
    let mut bsa = xedit_io::Archive::new();
    let list: Vec<PackFile> = files
        .iter()
        .enumerate()
        .map(|(index, (name, _))| PackFile {
            name: (*name).to_owned(),
            file_object: index,
            compress: false,
        })
        .collect();
    bsa.create_archive(&path.display().to_string(), ArchiveType::Sse, &list)
        .unwrap();
    for (name, data) in files {
        bsa.pack(name, data).unwrap();
    }
    bsa.save().unwrap();
}

/// The data folder: the plugin, its tables in `Loc - Strings.bsa`, a
/// `Loc.bsa`, an archive the game ini names and a My Games folder with that
/// ini.
fn setup(tag: &str) -> (TempDir, PathBuf) {
    let root = TempDir::new(format!("xedit-resources-{tag}-{}", std::process::id()));
    let data = root.join("Data");
    let my_games = root.join("My Games");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&my_games).unwrap();
    std::fs::write(data.join("Loc.esm"), plugin()).unwrap();
    archive(
        &data.join("Loc - Strings.bsa"),
        &[
            ("strings\\loc_english.strings", table(Some("From the archive"))),
            ("strings\\loc_english.dlstrings", table(None)),
            ("strings\\loc_english.ilstrings", table(None)),
        ],
    );
    archive(&data.join("Loc.bsa"), &[("meshes\\loc.nif", b"plugin".to_vec())]);
    archive(&data.join("Extra.bsa"), &[("meshes\\loc.nif", b"ini".to_vec())]);
    std::fs::write(
        my_games.join("Skyrim.ini"),
        "[Archive]\r\nsResourceArchiveList=Extra.bsa, Missing.bsa\r\n",
    )
    .unwrap();
    (root, data)
}

fn load(data: &Path, my_games: PathBuf, load_bsas: Option<bool>) -> Session {
    set_options(ResourceOptions {
        load_bsas,
        my_games: Some(format!("{}\\", my_games.display())),
        game_ini: None,
        custom_ini: None,
    });
    let path = format!("{}\\Loc.esm", data.display());
    let session = Session::load("sse", &[path]).unwrap();
    set_options(ResourceOptions::default());
    session
}

fn call(session: &mut Session, name: &str, params: Value) -> Value {
    Registry::standard()
        .call(session, name, params)
        .unwrap_or_else(|error| panic!("{name}: {error:?}"))
}

#[test]
fn a_session_loads_the_archives_of_the_game_ini_and_of_its_plugins() {
    let _lock = test_lock();
    let (root, data) = setup("load");
    let mut session = load(&data, root.join("My Games"), None);
    let data_name = format!("{}\\", data.display());
    assert_eq!(
        xedit_core::container_handler::container_list(),
        [
            format!("{data_name}Extra.bsa"),
            format!("{data_name}Loc - Strings.bsa"),
            format!("{data_name}Loc.bsa"),
            data_name.clone(),
        ]
    );
    // The archive added last that has the file wins, and the data folder
    // would win over both.
    assert_eq!(
        xedit_core::container_handler::open_resource_data("", "meshes\\loc.nif"),
        b"plugin"
    );
    assert!(xedit_core::container_handler::loader_done());
    // The tables come from `Loc - Strings.bsa`.
    let files = call(&mut session, "localization.files", json!({}));
    let tables = files["tables"].as_array().unwrap();
    assert_eq!(tables.len(), 3, "{files}");
    let got = call(
        &mut session,
        "localization.get",
        json!({"table": "Loc_English.STRINGS", "id": "00000001"}),
    );
    assert_eq!(got["text"], "From the archive");
}

#[test]
fn skip_bsa_names_the_archives_and_loads_the_data_folder_only() {
    let _lock = test_lock();
    let (root, data) = setup("skip");
    let mut session = load(&data, root.join("My Games"), Some(false));
    assert_eq!(
        xedit_core::container_handler::container_list(),
        [format!("{}\\", data.display())]
    );
    let files = call(&mut session, "localization.files", json!({}));
    assert_eq!(files["tables"].as_array().unwrap().len(), 0, "{files}");
}
