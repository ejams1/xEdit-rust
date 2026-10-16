// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The session a `script.run` test needs: a master with game settings and a
//! plugin with an override, a scripts folder of its own, and the helpers the
//! phase-6 step tests share. The same shapes as `tests/script.rs`, in one
//! module so the step worktrees do not all edit that file.

#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;

use serde_json::json;
use xedit_core::implementation::{FileBytes, FileImpl, wb_file_from_bytes};
use xedit_core::interface::globals::GameMode;
use xedit_core::interface::types::FileStates;
use xedit_io::temp::TempDir;
use xedit_session::{Registry, Session};

/// A folder of its own for a test, made fresh.
pub fn temp_dir(name: &str) -> TempDir {
    TempDir::new(format!("xedit-script-misc-{name}-{}", std::process::id()))
}

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

fn group(label: u32, group_type: i32, records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(&label.to_le_bytes());
    bytes.extend_from_slice(&group_type.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

fn header(flags: u32, record_count: u32, next_object_id: u32, masters: &[&str]) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&record_count.to_le_bytes());
    hedr.extend_from_slice(&next_object_id.to_le_bytes());
    let mut data = sub_record(b"HEDR", &hedr);
    data.extend(sub_record(b"CNAM", b"xedit-rust\0"));
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        data.extend(sub_record(b"MAST", &name));
        data.extend(sub_record(b"DATA", &0u64.to_le_bytes()));
    }
    data.extend(sub_record(b"INCC", &0u32.to_le_bytes()));
    main_record(b"TES4", flags, 0, &data)
}

/// A game setting with an editor ID.
fn gmst(form_id: u32, editor_id: &[u8]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", &1.5f32.to_le_bytes()));
    main_record(b"GMST", 0, form_id, &data)
}

fn open(name: &str, bytes: Vec<u8>) -> Arc<FileImpl> {
    wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap()
}

/// A session over a master with two game settings and a plugin with one
/// override and one of its own: four main records in all.
pub fn open_session() -> Session {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let mut master = header(1, 2, 0x802, &[]);
    master.extend(group(
        u32::from_le_bytes(*b"GMST"),
        0,
        &[gmst(0x800, b"fMaster\0"), gmst(0x801, b"fOther\0")].concat(),
    ));
    let mut plugin = header(0, 2, 0x802, &["Master.esm"]);
    plugin.extend(group(
        u32::from_le_bytes(*b"GMST"),
        0,
        &[gmst(0x0100_0800, b"fOverride\0"), gmst(0x0100_0801, b"fPlugin\0")].concat(),
    ));
    let master = open("Master.esm", master);
    let plugin = open("Plugin.esp", plugin);
    let mut session = Session::with_files(GameMode::gmSSE, vec![master, plugin]);
    session.allow_edit(true);
    session
}

/// A scripts folder of its own for a test, holding the script.
pub fn script_folder(name: &str, file: &str, text: &str) -> TempDir {
    let dir = temp_dir(name);
    std::fs::write(dir.join(file), text).unwrap();
    dir
}

/// Runs a script through `script.run` and gives its response.
pub fn run(session: &mut Session, folder: &Path, file: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut params = json!({
        "file": file,
        "scripts": folder.display().to_string(),
        "save": false,
    });
    for (key, value) in extra.as_object().unwrap() {
        params[key] = value.clone();
    }
    Registry::standard()
        .call(session, "script.run", params)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// The messages without the `[mm:ss]` prefixes the lines of the action carry.
pub fn lines(result: &serde_json::Value) -> Vec<String> {
    result["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| {
            let line = line.as_str().unwrap();
            match line.strip_prefix('[') {
                Some(rest) => match rest.split_once("] ") {
                    Some((time, text)) if time.contains(':') => text.to_owned(),
                    _ => line.to_owned(),
                },
                None => line.to_owned(),
            }
        })
        .collect()
}

/// The messages of a run that aborts: the failure lines end the log.
pub fn abort_text(result: &serde_json::Value) -> String {
    result["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `test_lock`: the session-wide lock the `script.run` tests hold, since
/// the run loop and the script globals are process-wide.
pub fn session_lock() -> std::sync::MutexGuard<'static, ()> {
    xedit_core::interface::globals::test_lock()
}
