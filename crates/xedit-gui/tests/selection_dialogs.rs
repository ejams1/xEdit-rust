// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The headless state of the step-5 selection dialogs over the real game
//! installs: the module tree from `modules.list`, the presets' round trip
//! through `modules.preset_*`, the filter options and their preset through
//! `signatures.list`/`filter.preset.save`/`settings.get`, and the
//! placement keys through `settings.get`/`settings.set`.
//!
//! The installs are read from `XEDIT_FO4_DATA` and `XEDIT_SSE_DATA`
//! (`source M:/projects/parity-env.sh`); a test skips itself when its game
//! is not installed. The globals are process-wide, so every test takes
//! `test_lock` and starts from an empty data path.

use eframe::egui;
use xedit_core::interface::globals::{GameMode, set_data_path, test_lock};
use xedit_gui::command::CommandBridge;
use xedit_gui::dialogs::filter_options_form::FilterOptionsState;
use xedit_gui::dialogs::mod_group_select_form::ModGroupSelectState;
use xedit_gui::dialogs::selection::DialogPlacement;
use xedit_gui::startup::{ModuleSelectMode, ModuleSelectState};
use xedit_session::Session;
use xedit_session::commands::SessionLoadRequest;
use xedit_session::filter::{FilterPresetSaveRequest, FilterPresetsRequest, SignaturesListRequest};
use xedit_session::module_select::{
    ModulesListRequest, ModulesListResponse, PresetDeleteRequest, PresetLoadRequest, PresetSaveRequest, PresetsRequest,
};
use xedit_session::settings::{SettingsGetRequest, SettingsSetRequest};

/// A scratch directory for one test. The project's scratch drive when it is
/// there (`M:\projects`), the platform's temporary directory otherwise (a
/// CI runner has no `M:`).
fn scratch(name: &str) -> std::path::PathBuf {
    let root = if std::path::Path::new(r"M:\projects").is_dir() {
        std::path::PathBuf::from(r"M:\projects\xedit-parity-scratch-p7-step5")
    } else {
        std::env::temp_dir().join(format!("xedit-parity-scratch-p7-step5-{}", std::process::id()))
    };
    let dir = root.join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A session over no files, after `selectable_modules` primed the game
/// paths and settings the command layer reads.
fn module_bridge(game: &str) -> Option<(CommandBridge, ModulesListResponse)> {
    set_data_path("");
    let selectable = xedit_session::module_select::selectable_modules(game).ok()?;
    let mode = selectable.game;
    let mut session = Session::with_files(mode, Vec::new());
    session.allow_edit(true);
    let mut bridge = CommandBridge::new(session);
    let response = bridge
        .request::<_, ModulesListResponse>(
            "modules.list",
            &ModulesListRequest {
                valid: true,
                active: false,
                masters_present: false,
            },
        )
        .expect("modules.list");
    Some((bridge, response))
}

#[test]
fn the_fo4_module_tree_comes_from_the_command() {
    if std::env::var_os("XEDIT_FO4_DATA").is_none() {
        eprintln!("skipping: XEDIT_FO4_DATA is not set");
        return;
    }
    let _lock = test_lock();
    let (mut bridge, list) = match module_bridge("fo4") {
        Some(pair) => pair,
        None => {
            eprintln!("skipping: the FO4 install was not found");
            return;
        }
    };
    assert!(!list.modules.is_empty());
    assert!(list.data_path.to_lowercase().contains("fallout 4"));
    let master = list
        .modules
        .iter()
        .find(|module| module.name.eq_ignore_ascii_case("Fallout4.esm"))
        .expect("Fallout4.esm");
    assert!(master.valid);
    assert_eq!(master.official_index, Some(i32::MIN));
    assert!(master.has_esm_flag);

    let mut state = ModuleSelectState::from_modules_list(list.clone(), ModuleSelectMode::Active, &[]);
    // `SelectFlag = mfActive`: the game master is checked.
    assert!(state.checked_names().iter().any(|name| name == "Fallout4.esm"));
    // `edFilterChange` and `cbRegExFilter`.
    state.filter.set_text("fallout4");
    state.refresh_rows();
    assert!(state.rows.iter().any(|row| row.name == "Fallout4.esm"));
    assert!(
        state
            .rows
            .iter()
            .all(|row| row.name.to_lowercase().contains("fallout4"))
    );
    state.filter.set_regex(true);
    state.filter.set_text("^Fallout4\\.esm$");
    state.refresh_rows();
    assert_eq!(state.rows.len(), 1);
    assert_eq!(state.rows[0].file_id, "00");
    assert_eq!(state.rows[0].load_order, "0");
    state.filter.set_text("");
    state.refresh_rows();

    // The OK list is `SimulateLoad`: full paths in load order.
    let paths = state.load_order().expect("a load order");
    assert!(paths.iter().any(|path| path.to_lowercase().ends_with("fallout4.esm")));
    assert!(
        paths
            .first()
            .is_some_and(|path| path.to_lowercase().ends_with("fallout4.esm"))
    );

    // `modules.preset_*` round trip over a private settings file.
    let dir = scratch("module-preset");
    let settings = dir.join("Plugins.fo4viewsettings");
    xedit_session::modgroups::set_file_options(None, Some(&settings.to_string_lossy()));
    let preset = "StepFiveTest".to_owned();
    let saved = bridge
        .request::<_, xedit_session::module_select::PresetSaveResponse>(
            "modules.preset_save",
            &PresetSaveRequest {
                preset: preset.clone(),
                modules: Some(state.checked_names()),
                after_modal: false,
                confirm: Some(true),
                dry_run: false,
            },
        )
        .expect("modules.preset_save");
    assert!(saved.written);
    let presets = bridge
        .request::<_, xedit_session::module_select::PresetsResponse>("modules.presets", &PresetsRequest {})
        .expect("modules.presets");
    assert!(presets.presets.contains(&preset));
    let loaded = bridge
        .request::<_, xedit_session::module_select::PresetLoadResponse>(
            "modules.preset_load",
            &PresetLoadRequest { preset: preset.clone() },
        )
        .expect("modules.preset_load");
    assert!(!loaded.preset_differs);
    assert_eq!(loaded.modules, state.filtered_names());
    state.apply_preset_load(&preset, &loaded.modules);
    let mut checked = state.checked_names();
    let mut expected = loaded.modules.clone();
    checked.sort();
    expected.sort();
    assert_eq!(checked, expected);
    let deleted = bridge
        .request::<_, xedit_session::module_select::PresetDeleteResponse>(
            "modules.preset_delete",
            &PresetDeleteRequest {
                preset: preset.clone(),
                confirm: Some(true),
                dry_run: false,
            },
        )
        .expect("modules.preset_delete");
    assert!(deleted.deleted);
    let presets = bridge
        .request::<_, xedit_session::module_select::PresetsResponse>("modules.presets", &PresetsRequest {})
        .expect("modules.presets");
    assert!(!presets.presets.contains(&preset));
}

#[test]
fn the_sse_signatures_and_filter_preset_round_trip() {
    if std::env::var_os("XEDIT_SSE_DATA").is_none() {
        eprintln!("skipping: XEDIT_SSE_DATA is not set");
        return;
    }
    let _lock = test_lock();
    set_data_path("");
    let selectable = match xedit_session::module_select::selectable_modules("sse") {
        Ok(selectable) => selectable,
        Err(error) => {
            eprintln!("skipping: {}", error.message);
            return;
        }
    };
    let mut session = Session::with_files(selectable.game, Vec::new());
    session.allow_edit(true);
    let mut bridge = CommandBridge::new(session);

    let signatures = bridge
        .request::<_, xedit_session::filter::SignaturesListResponse>("signatures.list", &SignaturesListRequest {})
        .expect("signatures.list");
    assert!(signatures.records.len() > 100);
    assert!(!signatures.base_records.is_empty());
    assert!(signatures.records.windows(2).all(|pair| pair[0].text <= pair[1].text));
    assert!(signatures.base_records.iter().any(|entry| entry.signature == "WEAP"));

    let dir = scratch("filter-preset");
    let settings = dir.join("Plugins.sseviewsettings");
    xedit_session::modgroups::set_file_options(None, Some(&settings.to_string_lossy()));
    let presets = bridge
        .request::<_, xedit_session::filter::FilterPresetsResponse>("filter.presets", &FilterPresetsRequest {})
        .expect("filter.presets");
    assert_eq!(presets.presets.first().map(String::as_str), Some(""));
    let mut state = FilterOptionsState::new(
        &signatures.records,
        &signatures.base_records,
        presets.presets.clone(),
        String::new(),
        false,
    );
    assert_eq!(state.conflict_all_list.items.len(), 6);
    assert_eq!(state.conflict_this_list.items.len(), 10);
    assert_eq!(state.record_signatures.items.len(), signatures.records.len());

    // Turn on a few options and round trip them through the preset file.
    state.by_editor_id = true;
    state.editor_id = "Player".to_owned();
    state.regex_comparison = true;
    state.record_signature = true;
    state.record_signatures.items[0].checked = true;
    state.conflict_all_list.items[0].checked = false;
    state.deleted = true;
    let options = state.to_options();
    let saved = bridge
        .request::<_, xedit_session::filter::FilterPresetSaveResponse>(
            "filter.preset.save",
            &FilterPresetSaveRequest {
                name: "StepFive".to_owned(),
                options: Some(options.clone()),
                dry_run: false,
            },
        )
        .expect("filter.preset.save");
    assert!(saved.changed);
    // `settings.get` is what `FilterLoadPreset` reads.
    let values = bridge
        .request::<_, xedit_session::settings::SettingsGetResponse>(
            "settings.get",
            &SettingsGetRequest {
                section: Some("Filter StepFive".to_owned()),
                key: None,
                value_type: None,
                default: None,
            },
        )
        .expect("settings.get");
    let values: Vec<(String, String)> = values.values.into_iter().map(|entry| (entry.key, entry.raw)).collect();
    let mut read = FilterOptionsState::new(
        &signatures.records,
        &signatures.base_records,
        presets.presets,
        "StepFive".to_owned(),
        false,
    );
    read.apply_preset_values("StepFive", &values);
    assert_eq!(read.to_options(), options);
    assert_eq!(read.preset.text, "StepFive");
    let deleted = bridge
        .request::<_, xedit_session::filter::FilterPresetDeleteResponse>(
            "filter.preset.delete",
            &xedit_session::filter::FilterPresetDeleteRequest {
                name: "StepFive".to_owned(),
                dry_run: false,
            },
        )
        .expect("filter.preset.delete");
    assert!(deleted.erased);
}

#[test]
fn a_private_mod_group_file_feeds_the_dialog_and_the_edit_state() {
    if std::env::var_os("XEDIT_FO4_DATA").is_none() {
        eprintln!("skipping: XEDIT_FO4_DATA is not set");
        return;
    }
    let _lock = test_lock();
    let (mut bridge, _) = match module_bridge("fo4") {
        Some(pair) => pair,
        None => {
            eprintln!("skipping: the FO4 install was not found");
            return;
        }
    };
    let dir = scratch("modgroups");
    let modgroups = dir.join("FO4Edit.modgroups");
    std::fs::write(&modgroups, "[StepFiveGroup]\nFallout4.esm\nDLCRobot.esm:DEADBEEF\n").unwrap();
    let settings = dir.join("Plugins.fo4viewsettings");
    xedit_session::modgroups::set_file_options(Some(&modgroups.to_string_lossy()), Some(&settings.to_string_lossy()));
    let list = bridge
        .request::<_, xedit_session::modgroups::ListResponse>(
            "modgroups.list",
            &xedit_session::modgroups::ListRequest { all: true },
        )
        .expect("modgroups.list");
    assert_eq!(list.mod_groups.len(), 1);
    let group = &list.mod_groups[0];
    assert_eq!(group.name, "StepFiveGroup");
    // No module is loaded in this session, so no item has its file.
    assert!(group.items.iter().all(|item| !item.has_file));

    let mut state = ModGroupSelectState::for_edit(list.mod_groups.clone(), "Which ModGroup do you want to edit?");
    assert_eq!(state.rows.len(), 1);
    assert!(state.rows[0].invalid);
    state.select_all(&[]);
    assert_eq!(state.checked_groups(), vec!["StepFiveGroup"]);
    assert!(state.error.is_none());

    let show = bridge
        .request::<_, xedit_session::modgroups::ShowResponse>(
            "modgroups.show",
            &xedit_session::modgroups::ShowRequest {
                name: "StepFiveGroup".to_owned(),
                file: None,
            },
        )
        .expect("modgroups.show");
    assert_eq!(
        show.lines,
        vec!["[StepFiveGroup]", "Fallout4.esm", "DLCRobot.esm:DEADBEEF"]
    );
    let modules = bridge
        .request::<_, ModulesListResponse>(
            "modules.list",
            &ModulesListRequest {
                valid: true,
                active: false,
                masters_present: false,
            },
        )
        .expect("modules.list");
    let mut edit = xedit_gui::dialogs::mod_group_edit_form::ModGroupEditState::new(
        &show,
        modules,
        "Editing ModGroup \"StepFiveGroup\"...",
    );
    assert!(edit.ok_enabled());
    assert_eq!(edit.lines(), vec!["Fallout4.esm", "DLCRobot.esm:DEADBEEF"]);
    // The edit round trip through `modgroups.edit` (a dry run: the file is
    // not written).
    edit.name = "StepFiveGroupRenamed".to_owned();
    edit.check_state();
    let change = bridge
        .request::<_, xedit_session::modgroups::ChangeResponse>(
            "modgroups.edit",
            &xedit_session::modgroups::EditRequest {
                name: "StepFiveGroup".to_owned(),
                file: None,
                new_name: Some(edit.name.clone()),
                items: Some(edit.lines()),
                add_current_crcs: false,
                dry_run: true,
            },
        )
        .expect("modgroups.edit");
    assert!(change.dry_run);
    assert!(change.reload.is_some());
}

#[test]
fn the_dialog_placement_round_trips_through_settings() {
    let _lock = test_lock();
    let dir = scratch("placement");
    let settings = dir.join("Plugins.testviewsettings");
    xedit_session::modgroups::set_file_options(None, Some(&settings.to_string_lossy()));
    let mut bridge = CommandBridge::new(Session::with_files(GameMode::gmFO4, Vec::new()));
    bridge.session_mut().allow_edit(true);

    let rect = egui::Rect::from_min_size(egui::pos2(12.0, 34.0), egui::vec2(640.0, 480.0));
    let placement = DialogPlacement::from_rect(rect);
    for (key, value) in placement.values() {
        bridge
            .request::<_, xedit_session::settings::SettingsSetResponse>(
                "settings.set",
                &SettingsSetRequest {
                    section: "frmFilterOptions".to_owned(),
                    key: key.to_owned(),
                    value: serde_json::json!(value),
                    value_type: Some("integer".to_owned()),
                    dry_run: false,
                },
            )
            .expect("settings.set");
    }
    let values = bridge
        .request::<_, xedit_session::settings::SettingsGetResponse>(
            "settings.get",
            &SettingsGetRequest {
                section: Some("frmFilterOptions".to_owned()),
                key: None,
                value_type: None,
                default: None,
            },
        )
        .expect("settings.get");
    let values: Vec<(String, String)> = values.values.into_iter().map(|entry| (entry.key, entry.raw)).collect();
    let read = DialogPlacement::from_values(&values).expect("the placement");
    assert_eq!(read.left, 12.0);
    assert_eq!(read.top, 34.0);
    assert_eq!(read.width, 640.0);
    assert_eq!(read.height, 480.0);
    // The session loads no plugins; the load request type is the CLI's.
    let _ = SessionLoadRequest {
        plugins: Vec::new(),
        compare_to: None,
        mode: None,
        force: false,
        dry_run: true,
    };
}
