// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The generated definition procedures run and register the records.

use xedit_core::interface::globals::{GameMode, set_game_mode, test_lock};
use xedit_core::interface::{Signature, clear_record_defs, find_record_def, record_defs};

#[test]
fn define_fo4_registers_the_records() {
    let _guard = test_lock();
    clear_record_defs();
    set_game_mode(GameMode::gmFO4);
    xedit_defs::fo4::define_fo4();
    let defs = record_defs();
    // The records that DefineFO4 defines for Fallout 4 without VR.
    assert_eq!(defs.len(), 147, "{} record definitions", defs.len());
    assert!(find_record_def(Signature::new(b"GMST")).is_some());
    assert!(find_record_def(Signature::new(b"NPC_")).is_some());
    clear_record_defs();
}
