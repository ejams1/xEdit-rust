// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The record panes over real plugins: the Referenced By filter and sort,
//! the element detail editor's validation and its `elements.set` commits,
//! the spreadsheet rows and the localization view.
//!
//! Two sources of plugins are used. The synthetic Skyrim SE master and
//! plugin exercise the reference index without a game install. The real
//! plugin is `DLCworkshop01.esm` from the local Fallout 4 install
//! (`XEDIT_FO4_DATA`, as for the parity harness), read from its bytes with
//! a stub Fallout4.esm master: only the DLC's own records are indexed, so
//! the tests stay fast. A test whose file is missing is skipped.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use xedit_core::implementation::{FileBytes, FileImpl, wb_file_from_bytes};
use xedit_core::interface::globals::GameMode;
use xedit_core::interface::types::FileStates;
use xedit_gui::command::CommandBridge;
use xedit_gui::element_detail::{ElementDetailPane, SelectedElement, ValueType};
use xedit_gui::panes::{
    LocalizationPane, RecordSelection, ReferencedByFilter, ReferencedByPane, SpreadsheetKind, SpreadsheetPane,
    pretty_form_id,
};
use xedit_session::Session;

// ---------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------

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

fn group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(label);
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

fn header(flags: u32, record_count: u32, next_object_id: u32, masters: &[&str]) -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.71f32.to_le_bytes());
    hedr.extend_from_slice(&record_count.to_le_bytes());
    hedr.extend_from_slice(&next_object_id.to_le_bytes());
    let mut data = sub_record(b"HEDR", &hedr);
    data.extend(sub_record(b"CNAM", b"xedit-gui\0"));
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        data.extend(sub_record(b"MAST", &name));
        data.extend(sub_record(b"DATA", &0u64.to_le_bytes()));
    }
    data.extend(sub_record(b"INCC", &0u32.to_le_bytes()));
    main_record(b"TES4", flags, 0, &data)
}

fn keyword(form_id: u32, editor_id: &[u8]) -> Vec<u8> {
    main_record(b"KYWD", 0, form_id, &sub_record(b"EDID", editor_id))
}

fn weapon(form_id: u32, editor_id: &[u8], full_name: &[u8]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"FULL", full_name));
    main_record(b"WEAP", 0, form_id, &data)
}

fn form_list(form_id: u32, editor_id: &[u8], entries: &[u32]) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    for entry in entries {
        data.extend(sub_record(b"LNAM", &entry.to_le_bytes()));
    }
    main_record(b"FLST", 0, form_id, &data)
}

/// The synthetic Skyrim SE master and plugin of the reference index tests:
/// a keyword referred to by a form list in the master, and a plugin that
/// overrides the list and adds one of its own.
fn synthetic_files() -> Vec<Arc<FileImpl>> {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    let mode = xedit_session::dump::setup_game("sse").expect("the Skyrim SE definitions load");
    xedit_session::save::apply_edit_settings(mode);
    let mut master = header(1, 2, 0x802, &[]);
    master.extend(group(b"KYWD", &keyword(0x800, b"KwMaster\0")));
    master.extend(group(b"FLST", &form_list(0x801, b"ListMaster\0", &[0x800])));
    let mut plugin = header(0, 5, 0x902, &["Master.esm"]);
    plugin.extend(group(b"KYWD", &keyword(0x0100_0900, b"KwPlugin\0")));
    let mut lists = form_list(0x801, b"ListMaster\0", &[0x800, 0x0100_0900]);
    lists.extend(form_list(0x0100_0901, b"ListPlugin\0", &[0x0100_0900, 0x800]));
    plugin.extend(group(b"FLST", &lists));
    let mut weapons = weapon(0x0100_0800, b"TestBlade\0", b"Test Blade\0");
    weapons.extend(weapon(0x0100_0801, b"TestBow\0", b"Test Bow\0"));
    plugin.extend(group(b"WEAP", &weapons));
    let master = wb_file_from_bytes("Master.esm", 0, FileStates::empty(), FileBytes::Owned(master))
        .expect("the synthetic master loads");
    let plugin = wb_file_from_bytes("Plugin.esp", 1, FileStates::empty(), FileBytes::Owned(plugin))
        .expect("the synthetic plugin loads");
    vec![master, plugin]
}

fn synthetic_bridge() -> CommandBridge {
    let mut session = Session::with_files(GameMode::gmSSE, synthetic_files());
    session.allow_edit(true);
    CommandBridge::new(session)
}

/// `DLCworkshop01.esm` read from its bytes, with a stub Fallout4.esm master
/// and without the real master's records: the reference index only has the
/// DLC's own records.
fn workshop_bridge() -> Option<&'static Mutex<CommandBridge>> {
    static BRIDGE: OnceLock<Option<Mutex<CommandBridge>>> = OnceLock::new();
    BRIDGE
        .get_or_init(|| {
            let data = std::env::var_os("XEDIT_FO4_DATA")?;
            let path = PathBuf::from(data).join("DLCworkshop01.esm");
            let bytes = std::fs::read(&path).ok()?;
            xedit_core::implementation::clear_files_map();
            xedit_core::implementation::reset_load_order_slots();
            xedit_core::interface::clear_files();
            xedit_session::refs::set_cache_options(None, true, true, true);
            let mode = xedit_session::dump::setup_game("fo4").ok()?;
            xedit_session::save::apply_edit_settings(mode);
            xedit_core::localization::install_localization_handler();
            let master = wb_file_from_bytes(
                "Fallout4.esm",
                0,
                FileStates::empty(),
                FileBytes::Owned(header(1, 0, 0x800, &[])),
            )
            .ok()?;
            let file = match wb_file_from_bytes("DLCworkshop01.esm", 1, FileStates::empty(), FileBytes::Owned(bytes)) {
                Ok(file) => file,
                Err(error) => {
                    eprintln!("skipping: DLCworkshop01.esm: {error}");
                    return None;
                }
            };
            let mut session = Session::with_files(GameMode::gmFO4, vec![master, file]);
            session.allow_edit(true);
            xedit_core::interface::globals::set_pretty_form_id(true);
            Some(Mutex::new(CommandBridge::new(session)))
        })
        .as_ref()
}

fn lock(bridge: &'static Mutex<CommandBridge>) -> MutexGuard<'static, CommandBridge> {
    bridge.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn skip(what: &str) -> bool {
    eprintln!("skipping: {what} is not available");
    true
}

/// The first record of the DLC and its editor ID.
fn first_record(bridge: &mut CommandBridge) -> (RecordSelection, String) {
    let records: serde_json::Value = bridge
        .call(
            "records.list",
            serde_json::json!({"file": "DLCworkshop01.esm", "limit": 1}),
        )
        .expect("the records list");
    let record = &records["records"][0];
    (
        RecordSelection::new(
            record["form_id"].as_str().unwrap(),
            Some("DLCworkshop01.esm".to_owned()),
        ),
        record["editor_id"].as_str().unwrap().to_owned(),
    )
}

// ---------------------------------------------------------------------
// The Referenced By tab
// ---------------------------------------------------------------------

/// The pane and `refs.get` agree on the filter, the sort and the caption.
fn referenced_by_against(bridge: &mut CommandBridge, selection: &RecordSelection, name: &str) {
    let mut pane = ReferencedByPane::new();
    pane.load(bridge, selection).expect("the list loads");
    let count = pane.total;
    assert!(count > 0, "{name} has references");
    assert_eq!(pane.items.len(), count);
    assert!(pane.visible());
    assert_eq!(pane.caption(false), format!("Referenced By ({count})"));

    let all = pane.items.clone();

    // The name filter: the command applies `ApplyReferencedByFilter`, and
    // every row it returns satisfies the pane's copy of the rule.
    let needle = all[0].name.to_lowercase();
    let needle = needle[..needle.len().min(6)].to_owned();
    pane.filter = ReferencedByFilter {
        name: needle,
        ..ReferencedByFilter::default()
    };
    pane.load(bridge, selection).expect("the filtered list loads");
    assert!(pane.items.iter().all(|item| pane.filter.matches(item)));
    let expected: Vec<&str> = all
        .iter()
        .filter(|item| pane.filter.matches(item))
        .map(|item| item.form_id.as_str())
        .collect();
    let actual: Vec<&str> = pane.items.iter().map(|item| item.form_id.as_str()).collect();
    assert_eq!(actual, expected, "refs.get filters as ApplyReferencedByFilter does");
    assert_eq!(pane.total, count, "the caption keeps ReferencedByCount");

    // The file filter is AND with the name or signature.
    pane.filter.or = true;
    pane.filter.file = "workshop".to_owned();
    pane.load(bridge, selection).expect("the AND/OR list loads");
    assert!(
        pane.items
            .iter()
            .all(|item| item.file.to_lowercase().contains("workshop"))
    );

    // Sorting is `CompareText` over the column; the caption names it.
    pane.filter = ReferencedByFilter::default();
    pane.load(bridge, selection).expect("the unfiltered list loads");
    let mut expected: Vec<String> = pane.items.iter().map(|item| item.name.clone()).collect();
    expected.sort_by(|left, right| xedit_core::localization::compare_text(left, right));
    // The last click sorts the Record column descending.
    expected.reverse();
    pane.on_column_click(0, false);
    assert_eq!(pane.sort.column, 3, "Record -> FormID");
    pane.on_column_click(0, true);
    assert_eq!(pane.sort.column, 0, "FormID -> Record");
    assert_eq!(pane.column_caption(0), "Record [Name] \u{25BC}");
    assert_eq!(
        pane.items.iter().map(|item| item.name.clone()).collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn the_referenced_by_tab_reads_the_synthetic_index() {
    let _guard = xedit_core::interface::globals::test_lock();
    let mut bridge = synthetic_bridge();
    let selection = RecordSelection::new("00000800", Some("Master.esm".to_owned()));
    referenced_by_against(&mut bridge, &selection, "KwMaster");
}

#[test]
fn the_referenced_by_tab_reads_a_real_plugin() {
    let _guard = xedit_core::interface::globals::test_lock();
    let Some(bridge) = workshop_bridge() else {
        skip("DLCworkshop01.esm");
        return;
    };
    let mut bridge = lock(bridge);
    // A record of the DLC has references among the DLC's own records.
    let records: serde_json::Value = bridge
        .call(
            "records.list",
            serde_json::json!({"file": "DLCworkshop01.esm", "limit": 400}),
        )
        .expect("the records list");
    let mut found = None;
    for record in records["records"].as_array().expect("an array") {
        let selection = RecordSelection::new(
            record["form_id"].as_str().unwrap(),
            Some("DLCworkshop01.esm".to_owned()),
        );
        let response: serde_json::Value = bridge
            .call(
                "refs.get",
                serde_json::json!({"form_id": selection.form_id, "file": selection.file}),
            )
            .expect("refs.get runs");
        if response["referenced_by_count"].as_u64().unwrap_or(0) > 0 {
            found = Some(selection);
            break;
        }
    }
    let selection = found.expect("a record of DLCworkshop01.esm has references");
    referenced_by_against(&mut bridge, &selection, "DLCworkshop01.esm");
}

// ---------------------------------------------------------------------
// The element detail editor
// ---------------------------------------------------------------------

#[test]
fn the_element_detail_editor_validates_and_commits_through_elements_set() {
    let _guard = xedit_core::interface::globals::test_lock();
    let Some(bridge) = workshop_bridge() else {
        skip("DLCworkshop01.esm");
        return;
    };
    let mut bridge = lock(bridge);
    bridge.session_mut().allow_edit(true);
    let (selection, original) = first_record(&mut bridge);

    let mut pane = ElementDetailPane::new();
    pane.select(Some(SelectedElement {
        record: selection,
        path: "EDID".to_owned(),
    }));
    pane.load(&mut bridge).expect("the element loads");
    assert_eq!(pane.edit_text, original);
    assert_eq!(pane.value_type(), Some(ValueType::Text));
    // An unchanged edit changes nothing.
    assert!(pane.commit(&mut bridge, true).unwrap());
    assert!(!pane.last_change.as_ref().unwrap().changed);

    // `vstViewNewText`: `Element.EditValue := NewText`, through
    // `elements.set`. A dry run applies nothing and reports no change.
    let edited = format!("{original}Test");
    pane.edit_text = edited.clone();
    assert!(pane.commit(&mut bridge, true).unwrap());
    assert!(!pane.last_change.as_ref().unwrap().changed);
    assert_eq!(
        pane.detail.as_ref().unwrap().node.value,
        original,
        "a dry run changes nothing"
    );
    assert!(pane.commit(&mut bridge, false).unwrap());
    assert!(pane.last_change.as_ref().unwrap().changed);
    assert_eq!(pane.edit_text, edited);
    assert_eq!(pane.last_change.as_ref().unwrap().file, "DLCworkshop01.esm");

    // Back to the value the record had.
    pane.edit_text = original.clone();
    assert!(pane.commit(&mut bridge, false).unwrap());
    assert_eq!(pane.edit_text, original);
}

// ---------------------------------------------------------------------
// The spreadsheets
// ---------------------------------------------------------------------

#[test]
fn the_spreadsheet_reads_every_record_of_the_group() {
    let _guard = xedit_core::interface::globals::test_lock();
    let mut bridge = synthetic_bridge();
    // The plugin's WEAP group is listed in full (the tab does not keep the
    // command's default page of 100), and the columns read the records.
    // A master is a file a command can name, as `files.list` shows it.
    let masters: serde_json::Value = bridge
        .call("records.list", serde_json::json!({"file": "Master.esm", "limit": 1}))
        .expect("the master's records are listed");
    assert_eq!(masters["total"], 2, "the keyword and the form list");
    let mut pane = SpreadsheetPane::new(SpreadsheetKind::Weapon);
    pane.load(&mut bridge, &["Plugin.esp".to_owned()], false)
        .expect("the spreadsheet loads");
    assert!(pane.loaded && !pane.columns.is_empty());
    assert_eq!(pane.rows.len(), 2, "both WEAP records of Plugin.esp");
    // The first row's EditorID column is the record's editor ID.
    let first = &pane.rows[0];
    assert_eq!(first.text(SpreadsheetKind::Weapon, 2, false), first.record.editor_id);
    // File Name and FormID are the first two columns.
    assert_eq!(pane.rows[0].text(SpreadsheetKind::Weapon, 0, false), "Plugin.esp");
    assert_eq!(
        pane.rows[0].text(SpreadsheetKind::Weapon, 1, false),
        pretty_form_id(&pane.rows[0].record.form_id)
    );
    // The Name column is the display text of the record.
    assert!(
        pane.rows
            .iter()
            .any(|row| row.text(SpreadsheetKind::Weapon, 3, false).contains("Test Blade"))
    );
    // A header click sorts the rows by that column (`vstSpreadSheetCompareNodes`).
    pane.sort_by_column(2, false);
}

// ---------------------------------------------------------------------
// The localization view
// ---------------------------------------------------------------------

#[test]
fn the_localization_view_is_empty_for_an_untranslated_plugin() {
    let _guard = xedit_core::interface::globals::test_lock();
    let Some(bridge) = workshop_bridge() else {
        skip("DLCworkshop01.esm");
        return;
    };
    let mut bridge = lock(bridge);
    let (selection, _) = first_record(&mut bridge);
    let mut pane = LocalizationPane::default();
    pane.load(&mut bridge, &selection).expect("the view loads");
    // The DLC is not a localized plugin: `mniViewEditClick` would not open
    // `TfrmLocalization` for any of its strings.
    assert!(pane.rows.is_empty());
    assert!(!pane.visible);
}
