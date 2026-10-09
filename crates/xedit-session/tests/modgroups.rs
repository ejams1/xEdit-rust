// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `modgroups.*` commands and the mod groups of `conflicts.list` on
//! synthetic Skyrim SE plugins in a data folder of their own: a master and
//! three plugins that each change the same global, and a fourth plugin that
//! is not loaded. Mod group files are written next to them, the program's
//! own file and the settings file are files of the folder too.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use xedit_core::implementation::{FileImpl, wb_file};
use xedit_core::interface::GameMode;
use xedit_core::interface::globals::{set_data_path, test_lock};
use xedit_core::interface::types::FileStates;
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

fn top_group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(label);
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

fn header(flags: u32, masters: &[&str]) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&1u32.to_le_bytes());
    hedr.extend_from_slice(&0x900u32.to_le_bytes());
    let mut data = sub_record(b"HEDR", &hedr);
    data.extend(sub_record(b"CNAM", b"xedit-rust\0"));
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        data.extend(sub_record(b"MAST", &name));
        data.extend(sub_record(b"DATA", &0u64.to_le_bytes()));
    }
    main_record(b"TES4", flags, 0, &data)
}

/// A plugin with the global `0x801` at `value`.
fn plugin(flags: u32, masters: &[&str], value: f32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", b"Shared\0");
    data.extend(sub_record(b"FNAM", b"f"));
    data.extend(sub_record(b"FLTV", &value.to_le_bytes()));
    let mut bytes = header(flags, masters);
    bytes.extend(top_group(b"GLOB", &main_record(b"GLOB", 0, 0x801, &data)));
    bytes
}

/// A data folder with `Base.esm`, `A.esp`, `B.esp`, `C.esp` (loaded) and
/// `D.esp` (not loaded), the mod group options pointing into it.
fn setup(test: &str) -> (PathBuf, Session) {
    let dir = std::env::temp_dir().join(format!("xedit-modgroups-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Base.esm"), plugin(1, &[], 1.0)).unwrap();
    for (i, name) in ["A.esp", "B.esp", "C.esp", "D.esp"].iter().enumerate() {
        std::fs::write(dir.join(name), plugin(0, &["Base.esm"], i as f32 + 2.0)).unwrap();
    }
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game_for_edit("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let files: Vec<Arc<FileImpl>> = ["Base.esm", "A.esp", "B.esp", "C.esp"]
        .iter()
        .map(|name| wb_file(dir.join(name).to_str().unwrap(), i32::MAX, FileStates::empty()).unwrap())
        .collect();
    set_data_path(&format!("{}\\", dir.display()));
    xedit_session::modgroups::set_file_options(
        Some(dir.join("global.modgroups").to_str().unwrap()),
        Some(dir.join("settings.ini").to_str().unwrap()),
    );
    let mut session = Session::with_files(GameMode::gmSSE, files);
    session.allow_edit(true);
    (dir, session)
}

fn call(session: &mut Session, name: &str, params: Value) -> Value {
    Registry::standard()
        .call(session, name, params)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn crc_of(dir: &Path, name: &str) -> String {
    format!("{:08X}", xedit_io::crc32(&std::fs::read(dir.join(name)).unwrap()))
}

/// The `conflict_this` of the global in each file.
fn statuses(session: &mut Session, params: Value) -> Vec<String> {
    let result = call(session, "conflicts.list", params);
    ["Base.esm", "A.esp", "B.esp", "C.esp"]
        .iter()
        .map(|file| {
            result["records"]
                .as_array()
                .unwrap()
                .iter()
                .find(|record| record["file"] == *file)
                .map(|record| record["conflict_this"].as_str().unwrap().to_owned())
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn an_active_mod_group_hides_the_records_of_its_targets() {
    let _guard = test_lock();
    let (dir, mut session) = setup("hide");
    std::fs::write(dir.join("B.modgroups"), "[A and B]\r\nA.esp\r\nB.esp\r\n").unwrap();
    let plain = statuses(&mut session, json!({}));
    assert_eq!(
        plain,
        ["ctMaster", "ctConflictLoses", "ctConflictLoses", "ctConflictWins"]
    );
    let hidden = statuses(&mut session, json!({ "mod_groups": ["a and b"] }));
    assert_eq!(
        hidden,
        ["ctMaster", "ctHiddenByModGroup", "ctConflictLoses", "ctConflictWins"]
    );
    let result = call(&mut session, "conflicts.list", json!({ "all_mod_groups": true }));
    assert_eq!(result["mod_groups"], json!(["A and B"]));
    assert_eq!(
        result["mod_group_hides"],
        json!([{ "file": "B.esp", "hides": ["A.esp"] }])
    );
    // The last record is never hidden: C hides B, but C is the last.
    std::fs::write(dir.join("C.modgroups"), "[B and C]\r\nB.esp\r\nC.esp\r\n").unwrap();
    let last = statuses(&mut session, json!({ "mod_groups": ["B and C"] }));
    assert_eq!(
        last,
        ["ctMaster", "ctConflictLoses", "ctHiddenByModGroup", "ctConflictWins"]
    );
    let error = Registry::standard()
        .call(&mut session, "conflicts.list", json!({ "mod_groups": ["nope"] }))
        .unwrap_err();
    assert_eq!(error.code, "unknown_mod_group");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn validity_follows_files_crcs_and_load_order() {
    let _guard = test_lock();
    let (dir, mut session) = setup("valid");
    let crc_a = crc_of(&dir, "A.esp");
    std::fs::write(
        dir.join("A.modgroups"),
        format!(
            "[Crc ok]\r\nA.esp:{crc_a}\r\nB.esp\r\n\
             [Crc wrong]\r\nA.esp:12345678\r\nB.esp\r\n\
             [Optional wrong]\r\n+A.esp:12345678\r\nB.esp\r\nC.esp\r\n\
             [Order]\r\nB.esp\r\nA.esp\r\n\
             [Forbidden]\r\n!C.esp\r\nA.esp\r\nB.esp\r\n\
             [Missing]\r\nA.esp\r\nD.esp\r\n\
             [Block]\r\nBase.esm\r\n{{B.esp\r\n{{A.esp\r\nC.esp\r\n\
             [Block bad]\r\nBase.esm\r\n{{C.esp\r\n{{A.esp\r\nB.esp\r\n\
             [Source only]\r\n#A.esp\r\nB.esp\r\n"
        ),
    )
    .unwrap();
    // A file of a module that is not loaded counts for nothing.
    std::fs::write(dir.join("D.modgroups"), "[Of D]\r\nA.esp\r\nB.esp\r\n").unwrap();
    let all = call(&mut session, "modgroups.list", json!({ "all": true }));
    let valid: Vec<(String, bool)> = all["mod_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| {
            (
                group["name"].as_str().unwrap().to_owned(),
                group["valid"].as_bool().unwrap(),
            )
        })
        .collect();
    let expect = [
        ("Block", true),
        ("Block bad", false),
        ("Crc ok", true),
        ("Crc wrong", false),
        ("Forbidden", false),
        ("Missing", false),
        ("Of D", true),
        ("Optional wrong", true),
        ("Order", false),
        ("Source only", false),
    ];
    let expect: Vec<(String, bool)> = expect
        .iter()
        .map(|(name, valid)| ((*name).to_owned(), *valid))
        .collect();
    assert_eq!(valid, expect);
    let listed = call(&mut session, "modgroups.list", json!({}));
    let names: Vec<&str> = listed["mod_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["name"].as_str().unwrap())
        .collect();
    // "Of D" is valid, but its file belongs to a module that is not loaded.
    assert_eq!(names, ["Block", "Crc ok", "Optional wrong"]);
    let show = call(&mut session, "modgroups.show", json!({ "name": "order" }));
    assert_eq!(
        show["mod_group"]["messages"],
        json!(["Error: \"A.esp\" has a lower load order than \"B.esp\""])
    );
    let show = call(&mut session, "modgroups.show", json!({ "name": "Block bad" }));
    assert_eq!(
        show["mod_group"]["messages"],
        json!(["Error: \"B.esp\" has a lower load order than \"C.esp\""])
    );
    let select = call(&mut session, "modgroups.select", json!({ "dry_run": true }));
    let messages = select["validation_messages"].as_array().unwrap();
    assert!(messages.contains(&json!("ModGroup \"Crc wrong\" is invalid:")));
    assert!(messages.contains(&json!(
        " - Error: Required module \"A.esp\" is present, but will be ignored as it doesn't match any of the specified CRC32s"
    )));
    assert!(messages.contains(&json!(
        " - Warning: Optional module \"A.esp\" is present, but will be ignored as it doesn't match any of the specified CRC32s"
    )));
    assert!(messages.contains(&json!(
        " - Hint: \"B.esp\" is ignored as a source as it has no valid targets above it"
    )));
    // The groups of the file of a module that is not loaded say nothing.
    assert!(!messages.iter().any(|line| line.as_str().unwrap().contains("Of D")));
    assert!(!dir.join("settings.ini").exists(), "a dry run writes nothing");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_selection_is_kept_in_the_settings_file() {
    let _guard = test_lock();
    let (dir, mut session) = setup("select");
    std::fs::write(
        dir.join("B.modgroups"),
        "[A and B]\r\nA.esp\r\nB.esp\r\n[Second, with comma]\r\nA.esp\r\nC.esp\r\n",
    )
    .unwrap();
    std::fs::write(dir.join("settings.ini"), "; mine\r\n[Other]\r\n key = value \r\n").unwrap();
    let result = call(
        &mut session,
        "modgroups.select",
        json!({ "names": ["A and B", "second, with comma"] }),
    );
    assert_eq!(result["selected"], json!(["A and B", "Second, with comma"]));
    assert_eq!(
        std::fs::read_to_string(dir.join("settings.ini")).unwrap(),
        "[Other]\r\nkey=value\r\n\r\n[ModGroups]\r\nSelection=\"A and B\",\"Second, with comma\"\r\n\r\n"
    );
    let saved = statuses(&mut session, json!({ "saved_mod_groups": true }));
    assert_eq!(
        saved,
        ["ctMaster", "ctHiddenByModGroup", "ctConflictLoses", "ctConflictWins"]
    );
    let listed = call(&mut session, "modgroups.list", json!({}));
    assert!(
        listed["mod_groups"]
            .as_array()
            .unwrap()
            .iter()
            .all(|group| group["selected"] == true)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn create_edit_delete_and_update_crcs_write_as_xedit() {
    let _guard = test_lock();
    let (dir, mut session) = setup("write");
    std::fs::write(
        dir.join("A.modgroups"),
        "; a comment\r\n[First]\r\n  +A.esp : 1234abcd , \r\n\r\nB.esp\r\n[Other]\r\nA.esp\r\nC.esp",
    )
    .unwrap();
    // Create: the group after an empty line, at the end of the file of the
    // module chosen, written as read (no preamble: the ANSI code page).
    let dry = call(
        &mut session,
        "modgroups.create",
        json!({ "name": "New", "modules": ["A.esp", "B.esp", "C.esp"], "file": "A.esp", "dry_run": true }),
    );
    assert_eq!(dry["dry_run"], true);
    let before = std::fs::read(dir.join("A.modgroups")).unwrap();
    call(
        &mut session,
        "modgroups.create",
        json!({ "name": "New", "modules": ["A.esp", "B.esp", "C.esp"], "file": "A.esp" }),
    );
    let mut expected = String::from_utf8(before).unwrap();
    expected.push_str("\r\n\r\n[New]\r\n@A.esp\r\nB.esp\r\n#C.esp\r\n");
    assert_eq!(std::fs::read_to_string(dir.join("A.modgroups")).unwrap(), expected);
    // The new group is selected.
    assert!(
        std::fs::read_to_string(dir.join("settings.ini"))
            .unwrap()
            .contains("Selection=New")
    );

    // Edit: the file read as an ini file without the group, the group
    // appended as written by `ToStrings`.
    call(
        &mut session,
        "modgroups.edit",
        json!({ "name": "first", "new_name": "Renamed" }),
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("A.modgroups")).unwrap(),
        "[Other]\r\nA.esp\r\nC.esp\r\n\r\n[New]\r\n@A.esp\r\nB.esp\r\n#C.esp\r\n\r\n[Renamed]\r\n+A.esp:1234ABCD\r\nB.esp\r\n"
    );

    // Update the CRC32s: every item of the groups that need it gets the
    // current CRC32 of its module.
    let (a, b, c) = (crc_of(&dir, "A.esp"), crc_of(&dir, "B.esp"), crc_of(&dir, "C.esp"));
    let update = call(
        &mut session,
        "modgroups.update_crcs",
        json!({ "mod_groups": ["Other"] }),
    );
    assert_eq!(update["message"], "One ModGroup has been updated.");
    assert_eq!(
        std::fs::read_to_string(dir.join("A.modgroups")).unwrap(),
        format!(
            "[New]\r\n@A.esp\r\nB.esp\r\n#C.esp\r\n\r\n[Renamed]\r\n+A.esp:1234ABCD\r\nB.esp\r\n\r\n[Other]\r\nA.esp:{a}\r\nC.esp:{c}\r\n"
        )
    );
    let update = call(&mut session, "modgroups.update_crcs", json!({}));
    assert_eq!(update["message"], "2 ModGroups have been updated.");
    let text = std::fs::read_to_string(dir.join("A.modgroups")).unwrap();
    assert!(
        text.contains(&format!("+A.esp:1234ABCD,{a}\r\nB.esp:{b}\r\n")),
        "{text}"
    );

    // Delete: the section erased and the ini written back.
    call(&mut session, "modgroups.delete", json!({ "names": ["New", "Renamed"] }));
    assert_eq!(
        std::fs::read_to_string(dir.join("A.modgroups")).unwrap(),
        format!("[Other]\r\nA.esp:{a}\r\nC.esp:{c}\r\n\r\n")
    );
    let nothing = call(&mut session, "modgroups.update_crcs", json!({}));
    assert_eq!(nothing["message"], "No ModGroups need updating.");
    let error = Registry::standard()
        .call(&mut session, "modgroups.delete", json!({ "names": ["Gone"] }))
        .unwrap_err();
    assert_eq!(error.code, "unknown_mod_group");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_utf8_file_keeps_its_preamble_on_delete_but_not_on_edit() {
    let _guard = test_lock();
    let (dir, mut session) = setup("bom");
    let text = "\u{feff}[One]\r\nA.esp\r\nB.esp\r\n[Two]\r\nA.esp\r\nC.esp\r\n";
    std::fs::write(dir.join("B.modgroups"), text).unwrap();
    call(&mut session, "modgroups.delete", json!({ "names": ["Two"] }));
    assert_eq!(
        std::fs::read(dir.join("B.modgroups")).unwrap(),
        b"\xEF\xBB\xBF[One]\r\nA.esp\r\nB.esp\r\n\r\n"
    );
    call(
        &mut session,
        "modgroups.edit",
        json!({ "name": "One", "items": ["A.esp", "+B.esp"] }),
    );
    assert_eq!(
        std::fs::read(dir.join("B.modgroups")).unwrap(),
        b"[One]\r\nA.esp\r\n+B.esp\r\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
