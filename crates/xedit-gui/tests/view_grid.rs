// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The view tab over real game data: the columns and rows of the view, and a
//! cell edit that reaches `elements.set` with the path the cell names.
//!
//! The tests read `XEDIT_SSE_DATA` and `XEDIT_FO4_DATA` and skip themselves
//! when the variable is not set, as the corpus tests of `xedit-session` do
//! (`source M:/projects/parity-env.sh` sets them). One session is loaded at
//! a time: the file list is process-wide.

use std::path::Path;
use std::sync::Mutex;

use serde_json::json;
use xedit_gui::command::CommandBridge;
use xedit_gui::settings::ColumnWidthMode;
use xedit_gui::view_grid::{ViewKind, ViewModel, ViewRecord, ViewSource};
use xedit_gui::widgets::grid::GridSource;
use xedit_session::Session;
use xedit_session::commands::ElementsSetResponse;
use xedit_session::conflicts::{CompareRequest, CompareResponse};

/// The file list is process-wide: one session per test binary at a time.
static LOCK: Mutex<()> = Mutex::new(());

/// Loads a session of `game` from `data` with the named plugins.
fn load(game: &str, data: &str, plugins: &[&str]) -> Session {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    let paths: Vec<String> = plugins
        .iter()
        .map(|name| Path::new(data).join(name).to_string_lossy().into_owned())
        .collect();
    let mut session = Session::load(game, &paths).expect("the game data loads");
    session.allow_edit(true);
    session
}

/// The SSE data folder, when the parity environment names one with the two
/// plugins the test needs.
fn sse_data() -> Option<String> {
    let data = std::env::var("XEDIT_SSE_DATA").ok()?;
    for name in ["Skyrim.esm", "Update.esm"] {
        if !Path::new(&data).join(name).exists() {
            return None;
        }
    }
    Some(data)
}

/// The FO4 data folder, when the parity environment names one.
fn fo4_data() -> Option<String> {
    let data = std::env::var("XEDIT_FO4_DATA").ok()?;
    Path::new(&data).join("Fallout4.esm").exists().then_some(data)
}

/// The `records.compare` request the view sends for one record.
fn compare_request(record: &ViewRecord) -> CompareRequest {
    CompareRequest {
        form_id: record.form_id.clone(),
        file: Some(record.file.clone()),
        master_and_leafs: false,
        hide_no_conflict: false,
        include_hidden: false,
        raw_data: false,
        mod_groups: Vec::new(),
        all_mod_groups: false,
        saved_mod_groups: false,
        view_filter_name: None,
        view_filter_value: None,
        view_filter_or: false,
        keep_children: false,
        keep_siblings: false,
        keep_parents_siblings: false,
        limit: None,
        depth: None,
        compact: false,
    }
}

/// The first record of `Update.esm` after the file header: an override of a
/// `Skyrim.esm` record, which the view shows with two or more columns.
fn first_update_record(bridge: &mut CommandBridge) -> ViewRecord {
    let listed = bridge
        .call("records.list", json!({ "file": "Update.esm", "offset": 1, "limit": 1 }))
        .expect("records.list answers");
    let record = &listed["records"][0];
    ViewRecord {
        form_id: record["form_id"].as_str().expect("a FormID").to_owned(),
        file: record["file"].as_str().expect("a file").to_owned(),
    }
}

#[test]
fn the_single_view_of_a_real_record_has_one_column_per_version() {
    let Some(data) = sse_data() else {
        return;
    };
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let session = load("sse", &data, &["Skyrim.esm", "Update.esm"]);
    let mut bridge = CommandBridge::new(session);
    let selected = first_update_record(&mut bridge);
    let response: CompareResponse = bridge
        .request("records.compare", &compare_request(&selected))
        .expect("records.compare answers");
    assert!(
        response.columns.len() >= 2,
        "{} has no override in the view",
        selected.form_id
    );
    let model = ViewModel::from_compare(&response, &selected);
    assert_eq!(model.kind, ViewKind::Single);
    // The columns are headed by the file names, master first (`DoSetActiveRecord`).
    assert_eq!(model.columns[0].title, "Skyrim.esm");
    assert!(model.columns.iter().any(|column| column.file == "Update.esm"));
    assert!(model.filler());
    assert_eq!(model.active_index, Some(1));
    // The rows are the members of the definition, with their children.
    assert!(!model.rows.is_empty());
    assert_eq!(model.rows[0].depth, 0);
    // Every non-empty cell of a record that has no children of its own
    // names a path the command layer can resolve.
    let paths: Vec<&str> = model
        .nodes
        .iter()
        .flat_map(|node| node.cells.iter())
        .filter_map(|cell| cell.path.as_deref())
        .collect();
    assert!(!paths.is_empty(), "no cell carries an element path");
}

#[test]
fn a_cell_edit_dispatches_elements_set_with_the_cell_path() {
    let Some(data) = sse_data() else {
        return;
    };
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let session = load("sse", &data, &["Skyrim.esm", "Update.esm"]);
    let mut bridge = CommandBridge::new(session);
    // The header of Update.esm: `session.info` gives each file's load order
    // file id (the high byte of the load order FormID of its records), and
    // the header's local ID is zero. Its CNAM subrecord is a plain editable
    // string (value "bnesmith" in Update.esm).
    let info: xedit_session::commands::SessionInfo = bridge
        .request("session.info", &xedit_session::NoParams {})
        .expect("session.info answers");
    let slot = info
        .files
        .iter()
        .zip(&info.file_ids)
        .find_map(|(file, id)| (file == "Update.esm").then_some(*id))
        .expect("Update.esm is loaded");
    assert!(slot >= 0, "Update.esm has no load order slot");
    let selected = ViewRecord {
        form_id: format!("{slot:02X}000000"),
        file: "Update.esm".to_owned(),
    };
    let response: CompareResponse = bridge
        .request("records.compare", &compare_request(&selected))
        .expect("records.compare answers");

    let mut model = ViewModel::from_compare(&response, &selected);
    let id_column = model
        .columns
        .iter()
        .position(|column| column.file == "Update.esm")
        .expect("the Update.esm column");
    let cells: Vec<String> = model
        .nodes
        .iter()
        .map(|view| {
            let cell = &view.cells[id_column];
            format!(
                "{}|{}|{:?}|{}",
                view.name,
                cell.path.as_deref().unwrap_or("-"),
                cell.editable,
                cell.value.as_deref().unwrap_or("-")
            )
        })
        .collect();
    let (node, path) = model
        .nodes
        .iter()
        .enumerate()
        .find_map(|(node, view)| {
            let cell = view.cells.get(id_column)?;
            // The element path is the element's display name (`CNAM - Author`).
            (cell.path.as_deref().is_some_and(|path| path.starts_with("CNAM")) && cell.editable)
                .then(|| (node, cell.path.clone().unwrap_or_default()))
        })
        .unwrap_or_else(|| panic!("the CNAM cell: {cells:#?}"));
    // The row index of the node in the visible rows (the tree is expanded).
    let row = model
        .rows
        .iter()
        .position(|row| row.node == node)
        .expect("the row is visible");
    let mut source = ViewSource::new(&mut model, 0, true, ColumnWidthMode::Standard, 900.0);
    assert!(source.set_cell(row, id_column + 1, "xedit-rust view test"));
    let edit = source.pending_edit.clone().expect("the edit is pending");
    assert_eq!(edit.path, path);
    assert_eq!(edit.file, "Update.esm");
    drop(source);
    let set: ElementsSetResponse = bridge
        .request(
            "elements.set",
            &xedit_session::commands::ElementsSetRequest {
                form_id: edit.form_id.clone(),
                path: edit.path.clone(),
                file: Some(edit.file.clone()),
                value: json!(edit.value),
                dry_run: false,
            },
        )
        .expect("elements.set answers");
    assert!(set.changed, "{} did not change", edit.path);
    let read = bridge
        .call(
            "elements.get",
            json!({
                "form_id": edit.form_id,
                "file": edit.file,
                "path": edit.path,
                "depth": 0,
            }),
        )
        .expect("elements.get answers");
    assert_eq!(read["value"], json!("xedit-rust view test"));
}

#[test]
fn a_navigation_file_row_shows_its_header_in_the_view() {
    let Some(data) = sse_data() else {
        return;
    };
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let session = load("sse", &data, &["Skyrim.esm", "Update.esm"]);
    let bridge = CommandBridge::new(session);
    let mut form = xedit_gui::TfrmMain::new(None, Vec::new(), true);
    form.bridge = Some(bridge);
    form.files = vec![
        "Skyrim.esm".to_owned(),
        "SkyrimSE.exe".to_owned(),
        "Update.esm".to_owned(),
    ];
    form.file_ids = vec![0, -1, 1];
    // `vstNavSelectionChanged` on the Update.esm row: its header record.
    form.activate_nav_file(2);
    assert_eq!(form.view.model.kind, ViewKind::Single, "{:?}", form.messages);
    assert_eq!(form.view.model.columns[0].file, "Update.esm");
    assert!(
        !form.view.model.rows.is_empty(),
        "the header view has no rows: {:?}",
        form.messages
    );
    // The hardcoded executable has no slot: the row refuses with a message.
    form.activate_nav_file(1);
    assert!(
        form.messages.lines.iter().any(|message| message.contains("hardcoded")),
        "{:?}",
        form.messages
    );
}

#[test]
fn the_fallout4_master_shows_its_definition_rows() {
    let Some(data) = fo4_data() else {
        return;
    };
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let session = load("fo4", &data, &["Fallout4.esm"]);
    let mut bridge = CommandBridge::new(session);
    // The header record of the master: TES4 rows, one column, no overrides.
    let selected = ViewRecord {
        form_id: "00000000".to_owned(),
        file: "Fallout4.esm".to_owned(),
    };
    let response: CompareResponse = bridge
        .request("records.compare", &compare_request(&selected))
        .expect("records.compare answers");
    let model = ViewModel::from_compare(&response, &selected);
    assert_eq!(model.kind, ViewKind::Single);
    assert_eq!(model.columns.len(), 1);
    assert!(!model.filler());
    // `DoSetActiveRecord`: `MemberCount + AdditionalElementCount` rows at the
    // top level, named by their definitions. The header subrecords show
    // their signature and name ("HEDR - Header"); a member the record does
    // not have (the master has no masters) shows the member's name.
    let names: Vec<&str> = model
        .rows
        .iter()
        .map(|row| model.nodes[row.node].name.as_str())
        .collect();
    assert!(names.contains(&"Record Header"), "{names:?}");
    assert!(names.contains(&"HEDR - Header"), "{names:?}");
    assert!(names.contains(&"Master Files"), "{names:?}");
    assert!(names.contains(&"CNAM - Author"), "{names:?}");
}
