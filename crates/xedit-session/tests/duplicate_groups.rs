// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fallout4.esm contains several top level groups twice (GRAS, TREE, FURN,
//! WEAP, AMMO, NPC_, LVLN, KEYM, ALCH, IDLM, NOTE, PROJ, HAZD and BNDS). The
//! oracle merges the earlier copy into the later one and sorts the result.
//! The test does nothing when `XEDIT_FO4_DATA` is not in the environment.

use std::path::Path;

use xedit_core::implementation::wb_file;
use xedit_core::interface::element::Container;
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::FileStates;

#[test]
fn merges_duplicated_top_level_groups() {
    let _guard = test_lock();
    let Some(data) = std::env::var("XEDIT_FO4_DATA").ok() else {
        return;
    };
    let path = format!("{data}\\Fallout4.esm");
    if !Path::new(&path).exists() {
        return;
    }
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_session::dump::setup_game("fo4").unwrap();
    let file = wb_file(&path, i32::MAX, FileStates::empty()).unwrap();

    let groups: Vec<String> = (0..file.get_element_count())
        .filter_map(|index| file.get_element(index))
        .map(|element| element.get_name())
        .collect();
    let bnds: Vec<usize> = groups
        .iter()
        .enumerate()
        .filter(|(_, name)| name.as_str() == "GRUP Top \"BNDS\"")
        .map(|(index, _)| index)
        .collect();
    assert_eq!(bnds.len(), 1, "one BNDS group after the merge: {groups:?}");
    // The merged group sits where the later copy was: after TERM.
    assert_eq!(groups[bnds[0] - 1], "GRUP Top \"HAZD\"");
    assert_eq!(groups[bnds[0] + 1], "GRUP Top \"LVLI\"");
    assert!(groups.iter().position(|name| name == "GRUP Top \"TERM\"").unwrap() < bnds[0]);

    // The records of both copies, sorted by FormID as the oracle dumps them.
    let group = file.get_element(bnds[0] as i32).unwrap();
    let group = group.as_container().unwrap();
    let form_ids: Vec<String> = (0..group.get_element_count())
        .filter_map(|index| group.get_element(index))
        .filter_map(|element| {
            element
                .as_main_record()
                .map(|record| record.get_fixed_form_id().to_string(true))
        })
        .collect();
    assert_eq!(
        form_ids,
        [
            "0001D971", "0001D972", "00021F34", "00035A21", "00037213", "0007CDAC", "00240C24", "00240C25", "00240C26",
            "00240C27", "00240C28", "00240C29",
        ]
    );
}
