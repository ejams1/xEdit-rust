// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The quick clean of synthetic Fallout 3 plugins with the PNAM fill of the
//! quick clean modes: the filter walk binds the nodes of a topic's
//! responses as it reaches them, after the sort that the conflict check of
//! a response without `PNAM` forces.

use std::sync::Arc;

use serde_json::json;
use xedit_core::implementation::{FileBytes, FileImpl, wb_file_from_bytes};
use xedit_core::interface::GameMode;
use xedit_core::interface::globals::{
    fill_inoa, fill_inom, fill_pnam, set_fill_inoa, set_fill_inom, set_fill_pnam, test_lock,
};
use xedit_core::interface::types::FileStates;
use xedit_io::temp::TempDir;
use xedit_session::{Registry, Session};

fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn main_record(signature: &[u8; 4], form_id: u32, data: &[u8]) -> Vec<u8> {
    record_with_flags(signature, 0, form_id, data)
}

fn record_with_flags(signature: &[u8; 4], flags: u32, form_id: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&form_id.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&15u16.to_le_bytes());
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

fn top_group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    group(u32::from_le_bytes(*label), 0, records)
}

fn header(flags: u32, record_count: u32, masters: &[&str]) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&0.94f32.to_le_bytes());
    hedr.extend_from_slice(&record_count.to_le_bytes());
    hedr.extend_from_slice(&0x900u32.to_le_bytes());
    let mut data = sub_record(b"HEDR", &hedr);
    data.extend(sub_record(b"CNAM", b"xedit-rust\0"));
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        data.extend(sub_record(b"MAST", &name));
        data.extend(sub_record(b"DATA", &0u64.to_le_bytes()));
    }
    record_with_flags(b"TES4", flags, 0, &data)
}

const QUEST: u32 = 0x800;
const TOPIC: u32 = 0x801;

/// A response of the quest with one line of text; `pnam` is the response
/// it follows (`Some(0)` is a `PNAM` of none, `None` no `PNAM` at all).
fn response(form_id: u32, pnam: Option<u32>, text: &str) -> Vec<u8> {
    let mut data = sub_record(b"DATA", &[0, 0, 0, 0]);
    data.extend(sub_record(b"QSTI", &QUEST.to_le_bytes()));
    if let Some(pnam) = pnam {
        data.extend(sub_record(b"PNAM", &pnam.to_le_bytes()));
    }
    let mut trdt = vec![0u8; 24];
    trdt[4] = 50;
    trdt[12] = 1;
    data.extend(sub_record(b"TRDT", &trdt));
    let mut text = text.as_bytes().to_vec();
    text.push(0);
    data.extend(sub_record(b"NAM1", &text));
    data.extend(sub_record(b"NAM2", b"\0"));
    data.extend(sub_record(b"NAM3", b"\0"));
    // The begin and end scripts, which the definition requires (a record
    // that lacks them gets them when it is built, an internal edit that
    // writes it from its elements).
    let mut schr = vec![0u8; 20];
    schr[18] = 1;
    data.extend(sub_record(b"SCHR", &schr));
    data.extend(sub_record(b"NEXT", &[]));
    data.extend(sub_record(b"SCHR", &schr));
    main_record(b"INFO", form_id, &data)
}

fn topic(responses: &[u8]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", b"TestTopic\0");
    data.extend(sub_record(b"QSTI", &QUEST.to_le_bytes()));
    data.extend(sub_record(b"DATA", &[0, 0]));
    let mut bytes = main_record(b"DIAL", TOPIC, &data);
    bytes.extend(group(TOPIC, 7, responses));
    top_group(b"DIAL", &bytes)
}

/// The master: a quest and a topic with three responses without `PNAM`.
fn master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 5, &[]);
    bytes.extend(top_group(
        b"QUST",
        &main_record(b"QUST", QUEST, &sub_record(b"EDID", b"TestQuest\0")),
    ));
    let mut responses = response(0x810, None, "First.");
    responses.extend(response(0x811, None, "Second."));
    responses.extend(response(0x812, None, "Third."));
    bytes.extend(topic(&responses));
    bytes
}

/// The plugin overrides the quest as it is in the master (an identical
/// record, which makes the clean save) and the three responses: the first
/// follows the third and its text ends with a blank, the second follows
/// none, and the third has no `PNAM` and a new text.
fn plugin_bytes() -> Vec<u8> {
    let mut bytes = header(0, 5, &["Master.esm"]);
    bytes.extend(top_group(
        b"QUST",
        &main_record(b"QUST", QUEST, &sub_record(b"EDID", b"TestQuest\0")),
    ));
    let mut responses = response(0x810, Some(0x812), "First. ");
    responses.extend(response(0x811, Some(0), "Second."));
    responses.extend(response(0x812, None, "Third, again."));
    bytes.extend(topic(&responses));
    bytes
}

fn load() -> (Vec<Arc<FileImpl>>, Session) {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game_for_edit("fo3").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmFO3);
    let mut files = Vec::new();
    for (name, bytes) in [("Master.esm", master_bytes()), ("Plugin.esp", plugin_bytes())] {
        files.push(wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap());
    }
    let mut session = Session::with_files(GameMode::gmFO3, files.clone());
    session.allow_edit(true);
    (files, session)
}

/// The NAM1 texts of the INFO records of a saved file, in file order.
fn texts_in(bytes: &[u8]) -> Vec<(u32, String)> {
    let mut result = Vec::new();
    let mut offset = 0;
    while offset + 24 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if &bytes[offset..offset + 4] == b"GRUP" {
            offset += 24;
            continue;
        }
        if &bytes[offset..offset + 4] == b"INFO" {
            let id = u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
            let data = &bytes[offset + 24..offset + 24 + size];
            let mut at = 0;
            while at + 6 <= data.len() {
                let length = u16::from_le_bytes(data[at + 4..at + 6].try_into().unwrap()) as usize;
                if &data[at..at + 4] == b"NAM1" {
                    let text = &data[at + 6..at + 6 + length];
                    result.push((id, String::from_utf8_lossy(&text[..text.len() - 1]).into_owned()));
                }
                at += 6 + length;
            }
        }
        offset += 24 + size;
    }
    result
}

/// The walk of the filter starts at the last response (the third, without
/// `PNAM`), binds the node before it, and the conflict check of the third
/// sorts the responses: the first now follows the second and the third,
/// so the node left to bind takes the second again and the first is never
/// compared. Its text keeps the blank that building it would trim
/// (`wbDialogueTextAfterLoad`), as the release writes `BrokenSteel.esm`'s
/// `GREETING` responses `000198DB` and `00025FC9`.
#[test]
fn a_response_the_sort_moves_past_the_walk_is_not_compared() {
    let _guard = test_lock();
    let saved = (fill_pnam(), fill_inom(), fill_inoa());
    set_fill_pnam(true);
    set_fill_inom(false);
    set_fill_inoa(false);
    let (_files, mut session) = load();
    let registry = Registry::standard();
    let dir = TempDir::new(format!("xedit-clean-topics-test-{}", std::process::id()));
    let output = dir.join("Plugin.esp");
    let result = registry.call(
        &mut session,
        "files.clean",
        json!({ "file": "Plugin.esp", "quick": true, "output": output.to_string_lossy(), "backup": false }),
    );
    set_fill_pnam(saved.0);
    set_fill_inom(saved.1);
    set_fill_inoa(saved.2);
    let result = result.unwrap();
    assert_eq!(result["passes"][0]["saved"]["written"], true, "{result:#}");
    let texts = texts_in(&std::fs::read(&output).unwrap());
    // The sorted order of the saved group: the second, the first, the third.
    assert_eq!(
        texts,
        [
            (0x811, "Second.".to_owned()),
            (0x810, "First. ".to_owned()),
            (0x812, "Third, again.".to_owned()),
        ],
        "{result:#}"
    );
}

/// A master whose topic holds its responses against the order their
/// `PNAM` links give (the first follows the third, the second none, the
/// third the second: sorted, the second, the third, the first), and a
/// plugin that overrides the topic alone.
fn sorting_master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 5, &[]);
    bytes.extend(top_group(
        b"QUST",
        &main_record(b"QUST", QUEST, &sub_record(b"EDID", b"TestQuest\0")),
    ));
    let mut responses = response(0x810, Some(0x812), "First.");
    responses.extend(response(0x811, Some(0), "Second."));
    responses.extend(response(0x812, Some(0x811), "Third."));
    bytes.extend(topic(&responses));
    bytes
}

fn topic_override_bytes() -> Vec<u8> {
    let mut bytes = header(0, 2, &["Master.esm"]);
    let mut data = sub_record(b"EDID", b"TestTopicChanged\0");
    data.extend(sub_record(b"QSTI", &QUEST.to_le_bytes()));
    data.extend(sub_record(b"DATA", &[0, 0]));
    bytes.extend(top_group(b"DIAL", &main_record(b"DIAL", TOPIC, &data)));
    bytes
}

/// The filter of an editing session (the `INOM` and `INOA` fills on, no
/// `PNAM` fill) asks for the conflict status of every record its first
/// pass reaches (`InheritConflictByParent`). The walk starts at the last
/// file, so the check of the plugin's topic builds the master's topic
/// first, and its `Init` sorts the master's responses before the walk
/// reaches their nodes: the nodes bind in the sorted order, as the
/// release's `vstNavInitNode` binds them (parity #41, the probe
/// `filter-topic-bind.pas` over `BrokenSteel.esm`). Binding every node
/// before the statuses were asked kept the file's order.
#[test]
fn the_filter_binds_a_topic_the_walk_sorted_in_the_sorted_order() {
    let _guard = test_lock();
    let saved = (fill_pnam(), fill_inom(), fill_inoa());
    set_fill_pnam(false);
    set_fill_inom(true);
    set_fill_inoa(true);
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game_for_edit("fo3").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmFO3);
    let mut files = Vec::new();
    for (name, bytes) in [
        ("Master.esm", sorting_master_bytes()),
        ("Plugin.esp", topic_override_bytes()),
    ] {
        files.push(wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap());
    }
    let mut session = Session::with_files(GameMode::gmFO3, files.clone());
    let registry = Registry::standard();
    let result = registry.call(
        &mut session,
        "filter.apply",
        json!({ "options": { "inherit_conflict_by_parent": true }, "list_records": true }),
    );
    set_fill_pnam(saved.0);
    set_fill_inom(saved.1);
    set_fill_inoa(saved.2);
    let result = result.unwrap();
    let records: Vec<String> = result["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record.as_str().unwrap().to_owned())
        .filter(|record| record.contains("INFO"))
        .collect();
    let order: Vec<&str> = records
        .iter()
        .map(|record| {
            ["00000810", "00000811", "00000812"]
                .into_iter()
                .find(|id| record.contains(id))
                .unwrap_or("?")
        })
        .collect();
    assert_eq!(order, ["00000811", "00000812", "00000810"], "{result:#}");
}

/// A plugin that overrides the topic with responses of its own, held against
/// the order of their `PNAM` links, as `BrokenSteel.esm` holds the responses
/// of `RadioGNRPlayerExploitsRELAY`.
fn own_responses_plugin_bytes() -> Vec<u8> {
    let mut bytes = header(0, 5, &["Master.esm"]);
    let mut responses = response(0x0100_0810, Some(0x0100_0812), "First.");
    responses.extend(response(0x0100_0811, Some(0), "Second."));
    responses.extend(response(0x0100_0812, Some(0x0100_0811), "Third."));
    let mut data = sub_record(b"EDID", b"TestTopic\0");
    data.extend(sub_record(b"QSTI", &QUEST.to_le_bytes()));
    data.extend(sub_record(b"DATA", &[0, 0]));
    let mut topic = main_record(b"DIAL", TOPIC, &data);
    topic.extend(group(TOPIC, 7, &responses));
    bytes.extend(top_group(b"DIAL", &topic));
    bytes
}

fn topic_master_bytes() -> Vec<u8> {
    let mut bytes = header(1, 2, &[]);
    bytes.extend(top_group(
        b"QUST",
        &main_record(b"QUST", QUEST, &sub_record(b"EDID", b"TestQuest\0")),
    ));
    let mut data = sub_record(b"EDID", b"TestTopic\0");
    data.extend(sub_record(b"QSTI", &QUEST.to_le_bytes()));
    data.extend(sub_record(b"DATA", &[0, 0]));
    bytes.extend(top_group(b"DIAL", &main_record(b"DIAL", TOPIC, &data)));
    bytes
}

/// After a filter the `Process` walk of a script follows the nodes of the
/// filtered tree with the elements they were bound to (`ApplyScriptToSelection`
/// over the filtered `vstNav`). The filter's walk binds the plugin's
/// responses in the file's order and only then checks the plugin's topic,
/// whose build sorts the group, so the walk visits the responses in the
/// file's order, not in the sorted order the group holds by then (parity
/// #41, `filter-topic-bind.pas`: `BrokenSteel.esm`'s
/// `RadioGNRPlayerExploitsRELAY`).
#[test]
fn the_walk_after_a_filter_follows_the_nodes_the_filter_bound() {
    let _guard = test_lock();
    let saved = (fill_pnam(), fill_inom(), fill_inoa());
    set_fill_pnam(false);
    set_fill_inom(true);
    set_fill_inoa(true);
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game_for_edit("fo3").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmFO3);
    let mut files = Vec::new();
    for (name, bytes) in [
        ("Master.esm", topic_master_bytes()),
        ("Plugin.esp", own_responses_plugin_bytes()),
    ] {
        files.push(wb_file_from_bytes(name, i32::MAX, FileStates::empty(), FileBytes::Owned(bytes)).unwrap());
    }
    let mut session = Session::with_files(GameMode::gmFO3, files.clone());
    session.allow_edit(true);
    let dir = TempDir::new(format!("xedit-filter-walk-test-{}", std::process::id()));
    std::fs::write(
        dir.join("FilterWalk.pas"),
        "unit userscript;\n\
         function Initialize: Integer;\n\
         begin\n\
           InheritConflictByParent := True;\n\
           ApplyFilter;\n\
           Result := 0;\n\
         end;\n\
         function Process(e: IInterface): Integer;\n\
         begin\n\
           if Signature(e) = 'INFO' then\n\
             AddMessage('info ' + IntToHex(GetLoadOrderFormID(e), 8));\n\
           Result := 0;\n\
         end;\n\
         end.\n",
    )
    .unwrap();
    let result = Registry::standard().call(
        &mut session,
        "script.run",
        json!({ "file": "FilterWalk.pas", "scripts": dir.display().to_string(), "save": false }),
    );
    set_fill_pnam(saved.0);
    set_fill_inom(saved.1);
    set_fill_inoa(saved.2);
    let result = result.unwrap();
    let visited: Vec<&str> = result["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|line| line.as_str()?.split_once("info ").map(|(_, id)| id))
        .collect();
    // The nodes from the last back to the first, in the file's order.
    assert_eq!(visited, ["01000812", "01000811", "01000810"], "{result:#}");
}
