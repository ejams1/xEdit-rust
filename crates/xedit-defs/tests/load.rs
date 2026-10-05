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
    let compressed = records
        .iter()
        .find(|record| record.header_struct().flags.is_compressed())
        .expect("a compressed record");
    assert!(compressed.data().is_some_and(|data| !data.is_empty()));
    clear_record_defs();
}
