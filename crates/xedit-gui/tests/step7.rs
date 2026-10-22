// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Phase 7 step 7: the wait form's cancellation, the options dialog's round
//! trip through `settings.get`/`settings.set`, and the two modal prompts
//! installed into the core seams.
//!
//! The commands run on synthetic Skyrim SE plugins (as the session's filter
//! tests do), so a cancelled operation stops at its first poll point without
//! touching the game installs.

use std::sync::Arc;

use serde_json::{Value, json};
use xedit_core::implementation::{FileImpl, wb_file};
use xedit_core::interface::globals::{GameMode, set_data_path, set_force_terminate, test_lock};
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{ElementRef, FormID, MainRecordRef};
use xedit_session::{Registry, Session};

use xedit_gui::dialogs::options_form::state_from_values;
use xedit_gui::dialogs::prompts::{self, PromptAnswer, PromptQueue, PromptRequest};
use xedit_gui::theme::ThemeChoice;
use xedit_gui::widgets::conflict::ConflictColors;

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

fn top_group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    group(u32::from_le_bytes(*label), 0, records)
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

fn gmst(form_id: u32, editor_id: &[u8], value: f32) -> Vec<u8> {
    let mut data = sub_record(b"EDID", editor_id);
    data.extend(sub_record(b"DATA", &value.to_le_bytes()));
    main_record(b"GMST", 0, form_id, &data)
}

/// The master: a game setting and an interior cell nothing refers to.
fn master_bytes() -> Vec<u8> {
    let mut cell = sub_record(b"EDID", b"TestCell\0");
    cell.extend(sub_record(b"DATA", &1u16.to_le_bytes()));
    let mut bytes = header(1, 3, 0x802, &[]);
    bytes.extend(top_group(b"GMST", &gmst(0x800, b"fMaster\0", 1.5)));
    let cells = group(9, 2, &group(4, 3, &main_record(b"CELL", 0, 0x801, &cell)));
    bytes.extend(top_group(b"CELL", &cells));
    bytes
}

/// The plugin: the master's game setting with another value, and one of its
/// own.
fn plugin_bytes() -> Vec<u8> {
    let mut bytes = header(0, 2, 0x802, &["Master.esm"]);
    bytes.extend(top_group(b"GMST", &gmst(0x0800, b"fMaster\0", 2.5)));
    bytes.extend(top_group(b"GMST", &gmst(0x0100_0802, b"fOther\0", 3.5)));
    bytes
}

/// The test's data folders, below the project's scratch folder (never the
/// system drive, per the project's rule).
struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(name: String) -> Self {
        let root = std::env::var_os("XEDIT_TEST_SCRATCH")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::path::Path::new("M:/projects/scratch")
                    .is_dir()
                    .then(|| std::path::PathBuf::from("M:/projects/scratch"))
            })
            .unwrap_or_else(std::env::temp_dir);
        let path = root.join("p7-step7-tests").join(name);
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }

    fn join(&self, name: &str) -> std::path::PathBuf {
        self.path.join(name)
    }

    fn to_string_lossy(&self) -> std::borrow::Cow<'_, str> {
        self.path.to_string_lossy()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct Files {
    master: Arc<FileImpl>,
    plugin: Arc<FileImpl>,
}

/// A data folder with a settings file of its own, so the presets and the
/// options do not touch the user's.
fn data_dir(test: &str) -> TempDir {
    let dir = TempDir::new(format!("xedit-step7-{test}-{}", std::process::id()));
    set_data_path(&dir.to_string_lossy());
    xedit_session::modgroups::set_file_options(None, Some(&dir.join("xedit.ini").to_string_lossy()));
    dir
}

fn load(dir: &TempDir) -> Files {
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    xedit_session::save::apply_edit_settings(GameMode::gmSSE);
    let open = |name: &str, bytes: Vec<u8>| {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        wb_file(&path.to_string_lossy(), i32::MAX, FileStates::empty()).unwrap()
    };
    let master = open("Master.esm", master_bytes());
    let plugin = open("Plugin.esp", plugin_bytes());
    Files { master, plugin }
}

fn session(files: &Files) -> (Registry, Session) {
    let mut session = Session::with_files(GameMode::gmSSE, vec![files.master.clone(), files.plugin.clone()]);
    session.allow_edit(true);
    (Registry::standard(), session)
}

/// Calls a command and returns its error code.
fn error_code(registry: &Registry, session: &mut Session, name: &str, params: Value) -> String {
    registry
        .call(session, name, params)
        .map(|_| String::new())
        .unwrap_or_else(|error| error.code)
}

/// The wait form of the test: `canCancel` and a flag.
struct TestWaitForm {
    canceled: std::sync::atomic::AtomicBool,
}

impl xedit_core::interface::wait_form::WaitForm for TestWaitForm {
    fn is_canceled(&self) -> bool {
        self.canceled.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn create_progress(
        &self,
        _caption: &str,
        _status: &str,
        _max: i64,
    ) -> Arc<dyn xedit_core::interface::wait_form::Progress> {
        struct NoProgress;
        impl xedit_core::interface::wait_form::Progress for NoProgress {
            fn update_status(&self, _position: i64, _status: &str) {}
        }
        Arc::new(NoProgress)
    }
}

fn clear_cancel() {
    set_force_terminate(false);
    xedit_core::interface::wait_form::reset();
}

#[test]
fn a_cancelled_filter_stops_at_the_next_poll_point() {
    let _guard = test_lock();
    let dir = data_dir("cancel-filter");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    // `wbForceTerminate` (the main form's cancel panel) stops the walk.
    set_force_terminate(true);
    let code = error_code(&registry, &mut session, "filter.apply", json!({ "list_records": true }));
    assert_eq!(code, "cancelled");
    clear_cancel();
}

#[test]
fn a_cancelled_wait_form_stops_the_command() {
    let _guard = test_lock();
    let dir = data_dir("cancel-form");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    // The job runner's form: `GetIsCanceled` is `fwCanCancel and fwCanceled`.
    let form = Arc::new(TestWaitForm {
        canceled: std::sync::atomic::AtomicBool::new(true),
    });
    xedit_core::interface::wait_form::set_current_wait_form(Some(form));
    let code = error_code(&registry, &mut session, "filter.apply", json!({ "list_records": true }));
    assert_eq!(code, "cancelled");
    clear_cancel();
}

#[test]
fn a_cancelled_clean_stops_before_the_walk() {
    let _guard = test_lock();
    let dir = data_dir("cancel-clean");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    set_force_terminate(true);
    let code = error_code(
        &registry,
        &mut session,
        "files.clean",
        json!({ "file": "Plugin.esp", "itm": true }),
    );
    assert_eq!(code, "cancelled");
    clear_cancel();
}

#[test]
fn a_cancelled_refs_build_stops_before_the_files() {
    let _guard = test_lock();
    let dir = data_dir("cancel-refs");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    set_force_terminate(true);
    let code = error_code(&registry, &mut session, "refs.build", json!({}));
    assert_eq!(code, "cancelled");
    clear_cancel();
}

#[test]
fn a_cancelled_lodgen_stops_before_the_resources() {
    let _guard = test_lock();
    let dir = data_dir("cancel-lod");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    set_force_terminate(true);
    let code = error_code(&registry, &mut session, "lodgen.generate", json!({ "dry_run": true }));
    assert_eq!(code, "cancelled");
    clear_cancel();
}

#[test]
fn the_options_round_trip_through_settings_get_and_set() {
    let _guard = test_lock();
    let dir = data_dir("options");
    let files = load(&dir);
    let (registry, mut session) = session(&files);
    // The dialog as the main form builds it.
    let mut state = state_from_values(
        &[],
        &xedit_gui::theme::Fonts::default(),
        &ConflictColors::default(),
        ThemeChoice::Dark,
        true,
    );
    state.set_toggle("HideUnused", false);
    state.set_toggle("CollapseRGBA", false);
    state.set_toggle("ShowFlagEnumValue", true);
    state.udr_set_scale = true;
    state.udr_set_scale_value = "0.5".to_owned();
    state.column_width = "321".to_owned();
    state.row_height = "22".to_owned();
    state.auto_compare_selected_limit = "7".to_owned();
    state.shrink_buttons = true;

    // `mniNavOptionsClick`'s writes.
    for (key, value, kind) in state.option_values() {
        // `WriteBool` takes a JSON boolean; the dialog holds "1"/"0".
        let value = if kind == "bool" {
            json!(value != "0")
        } else {
            json!(value)
        };
        registry
            .call(
                &mut session,
                "settings.set",
                json!({ "section": "Options", "key": key, "value": value, "value_type": kind }),
            )
            .unwrap();
    }
    for (section, key, value) in state.color_values() {
        registry
            .call(
                &mut session,
                "settings.set",
                json!({ "section": section, "key": key, "value": value, "value_type": "integer" }),
            )
            .unwrap();
    }
    for (key, value, kind) in state.ui_values() {
        registry
            .call(
                &mut session,
                "settings.set",
                json!({ "section": "UI", "key": key, "value": value, "value_type": kind }),
            )
            .unwrap();
    }

    // `settings.get` of the section, as the dialog reads it.
    let read: Value = registry
        .call(
            &mut session,
            "settings.get",
            json!({ "section": "Options", "value_type": "string" }),
        )
        .unwrap();
    let values: Vec<(String, String)> = read["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["key"].as_str().unwrap().to_owned(),
                entry["raw"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let read_back = state_from_values(
        &values,
        &xedit_gui::theme::Fonts::default(),
        &ConflictColors::default(),
        ThemeChoice::Dark,
        true,
    );
    assert!(!read_back.toggle("HideUnused"));
    assert!(!read_back.toggle("CollapseRGBA"));
    assert!(read_back.toggle("ShowFlagEnumValue"));
    assert!(read_back.udr_set_scale);
    assert_eq!(read_back.udr_set_scale_value, "0.5");
    assert_eq!(read_back.column_width, "321");
    assert_eq!(read_back.row_height, "22");
    assert_eq!(read_back.auto_compare_selected_limit, "7");
    assert!(read_back.shrink_buttons);
    // Every key the dialog writes survives the trip.
    assert_eq!(read_back.option_values(), state.option_values());

    // The colours: `[ColorConflictThis]`/`[ColorConflictAll]` by enum name.
    let colors: Value = registry
        .call(
            &mut session,
            "settings.get",
            json!({ "section": "ColorConflictAll", "key": "caConflict", "value_type": "integer" }),
        )
        .unwrap();
    // `clRed` written as its `TColor`.
    assert_eq!(colors["values"][0]["value"], 0x0000_00FF);
    let theme: Value = registry
        .call(&mut session, "settings.get", json!({ "section": "UI", "key": "Theme" }))
        .unwrap();
    assert_eq!(theme["values"][0]["raw"], "Dark");
}

#[test]
fn the_form_id_prompt_answers_the_core_seam() {
    let _guard = test_lock();
    let dir = data_dir("prompt-formid");
    let files = load(&dir);
    // `allow_edit` sets `wbEditAllowed`, which the file header's
    // `Next Object ID` needs to be writable.
    let (_registry, _session) = session(&files);
    let queue = Arc::new(PromptQueue::new());
    prompts::install(&queue);
    let file = files.plugin.clone();
    let element: ElementRef = file.clone();
    let asker = std::thread::spawn(move || xedit_core::interface::globals::wb_get_form_id(&element));
    // The frame loop pops the question and answers it.
    let pending = loop {
        if let Some(pending) = queue.pop() {
            break pending;
        }
        std::thread::yield_now();
    };
    match &pending.request {
        PromptRequest::FormId { suggested } => {
            // `FileFormIDtoLoadOrderFormID(NewFormID, True)` of the plugin:
            // `Next Object ID` is $802, which the plugin's own game setting
            // takes, so the new FormID is $803 of the plugin's slot.
            assert_eq!(suggested, "01000803");
        }
        other => panic!("unexpected {other:?}"),
    }
    let given = FormID::from_str("0100ABCD").unwrap();
    pending.answer(PromptAnswer::FormId(Some(given)));
    assert_eq!(asker.join().unwrap(), given);
    prompts::uninstall();
}

#[test]
fn a_cancelled_form_id_prompt_restores_the_next_object_id() {
    let _guard = test_lock();
    let dir = data_dir("prompt-cancel");
    let files = load(&dir);
    let (_registry, _session) = session(&files);
    let queue = Arc::new(PromptQueue::new());
    prompts::install(&queue);
    let before = files.plugin.get_next_object_id();
    let element: ElementRef = files.plugin.clone();
    let asker = std::thread::spawn(move || xedit_core::interface::globals::wb_get_form_id(&element));
    let pending = loop {
        if let Some(pending) = queue.pop() {
            break pending;
        }
        std::thread::yield_now();
    };
    pending.answer(PromptAnswer::FormId(None));
    // `if (Result.IsNull) and (ObjectID <> 0) then aElement._File.NextObjectID
    // := ObjectID`.
    assert_eq!(asker.join().unwrap(), FormID::null());
    assert_eq!(files.plugin.get_next_object_id(), before);
    prompts::uninstall();
}

#[test]
fn the_cell_details_prompt_answers_the_core_seam() {
    let _guard = test_lock();
    let dir = data_dir("prompt-cell");
    let files = load(&dir);
    let queue = Arc::new(PromptQueue::new());
    prompts::install(&queue);
    // The worldspace is only the question's caption; the seam's signature
    // takes the record.
    let record = files
        .master
        .record_by_form_id(FormID::from_cardinal(0x801), true, true)
        .unwrap();
    let world: MainRecordRef = record;
    let asker = std::thread::spawn(move || xedit_core::interface::globals::wb_get_cell_details_for_worldspace(&world));
    let pending = loop {
        if let Some(pending) = queue.pop() {
            break pending;
        }
        std::thread::yield_now();
    };
    match &pending.request {
        PromptRequest::CellDetails { worldspace } => assert!(!worldspace.is_empty()),
        other => panic!("unexpected {other:?}"),
    }
    pending.answer(PromptAnswer::CellDetails(Some((false, 4, -2))));
    assert_eq!(asker.join().unwrap(), Some((false, 4, -2)));
    prompts::uninstall();
}
