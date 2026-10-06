// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loading a plugin of an installed game. The tests do nothing when the
//! game data path is not in the environment.

use std::path::Path;

use xedit_core::implementation::{reset_load_order_slots, wb_file};
use xedit_core::interface::globals::{GameMode, set_game_mode, test_lock};
use xedit_core::interface::{Container, Element, ElementType, FileStates, MainRecord, Signature, clear_record_defs};

/// The path of a plugin of the game, or `None` when the game is not installed here.
fn plugin(data_env: &str, name: &str) -> Option<String> {
    let data = std::env::var(data_env).ok()?;
    let path = format!("{data}/{name}");
    Path::new(&path).exists().then_some(path)
}

#[test]
fn scans_the_structure_of_a_skyrim_plugin() {
    let _guard = test_lock();
    let Some(path) = plugin("XEDIT_SSE_DATA", "Update.esm") else {
        return;
    };
    clear_record_defs();
    set_game_mode(GameMode::gmSSE);
    xedit_defs::tes5::define_tes5();
    reset_load_order_slots();
    let file = wb_file(&path, 0, FileStates::empty()).unwrap();
    let header = file.header().unwrap();
    assert_eq!(header.get_signature(), Signature::new(b"TES4"));
    assert!(header.get_version() >= 40, "version {}", header.get_version());
    assert!(
        file.get_element_count() > 10,
        "{} top level elements",
        file.get_element_count()
    );
    let records = file.records();
    assert!(records.len() > 1000, "{} records", records.len());
    // Every record after the header belongs to a group and has a definition.
    for record in records.iter().skip(1) {
        assert_eq!(
            record.get_container().unwrap().get_element_type(),
            ElementType::etGroupRecord
        );
        assert!(record.def().is_some(), "no definition for {}", record.get_name());
    }
    // The game settings have their subrecords grouped by the definition,
    // their editor ID read, and their value shown by the definition.
    let mut values = Vec::new();
    for record in records
        .iter()
        .filter(|record| record.get_signature() == Signature::new(b"GMST"))
    {
        assert!(
            record.get_element_count() >= 2,
            "{} has {} subrecords",
            record.get_name(),
            record.get_element_count()
        );
        assert!(
            !record.get_editor_id().is_empty(),
            "{} has no editor ID",
            record.get_name()
        );
        // The value of the union is a child element of the subrecord.
        let data = record.get_element_by_name("DATA - Value").expect("a DATA subrecord");
        let value = data
            .as_container()
            .and_then(|data| data.get_element(0))
            .expect("the value element");
        values.push(format!("{} = {}", record.get_editor_id(), value.get_value()));
    }
    assert!(values.contains(&"fDiffMultHPToPCL = 3.000000".to_owned()), "{values:?}");
    assert!(values.contains(&"iUpdateESMVersion = 8".to_owned()), "{values:?}");
    let compressed = records
        .iter()
        .find(|record| record.header_struct().flags.is_compressed())
        .expect("a compressed record");
    assert!(compressed.data().is_some_and(|data| !data.is_empty()));
    clear_record_defs();
}

/// Every record of the plugin builds its subrecords. Ignored until the
/// values and the deciders that need them are ported.
#[test]
#[ignore]
fn initializes_every_record_of_a_skyrim_plugin() {
    let _guard = test_lock();
    let Some(path) = plugin("XEDIT_SSE_DATA", "Update.esm") else {
        return;
    };
    clear_record_defs();
    set_game_mode(GameMode::gmSSE);
    xedit_defs::tes5::define_tes5();
    reset_load_order_slots();
    let file = wb_file(&path, 0, FileStates::empty()).unwrap();
    for record in file.records() {
        record.get_element_count();
    }
    clear_record_defs();
}

#[test]
fn placed_record_names_its_base() {
    let _guard = test_lock();
    let Some(path) = plugin("XEDIT_SSE_DATA", "Update.esm") else {
        return;
    };
    clear_record_defs();
    xedit_core::implementation::clear_files_map();
    set_game_mode(GameMode::gmSSE);
    xedit_defs::tes5::define_tes5();
    let file = wb_file(&path, i32::MAX, FileStates::empty()).unwrap();
    let refr = file
        .record_by_form_id(xedit_core::interface::FormID::from_cardinal(0x0002C46C), true, true)
        .expect("the REFR");
    let names: Vec<String> = (0..refr.get_element_count())
        .filter_map(|index| refr.get_element(index))
        .map(|element| element.get_name())
        .collect();
    assert!(names.iter().any(|name| name.starts_with("NAME")), "{names:?}");
    assert!(refr.get_name().contains("Places"), "{}", refr.get_name());
}
