// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsFNVSaves.pas

//! The callbacks of `wbDefinitionsFNVSaves.pas` that are ported by hand.
//! The ones that are not ported yet are stubs in `fnvsaves_stubs.rs`.

#[allow(unused_imports)]
pub use super::fnvsaves_stubs::*;

use xedit_core::interface::globals::{
    set_extract_info, set_file_chapters, set_file_header, set_file_magic, set_file_plugins,
};
use xedit_core::interface::*;

use crate::fnv::define_fnv;
use crate::fnvsaves::{WB_ACTOR_VALUE_LABELS, WB_CO_SAVE_CHAPTERS, WB_CO_SAVE_HEADER, define_fnv_saves_s};

/// Upstream `ExtractInfoSave`: the chapters of a save that are initialized
/// while it loads.
const EXTRACT_INFO_SAVE: [u8; 2] = [3, 4];

/// Port of `DefineFNVSavesA`: the labels of the actor value arrays.
pub fn define_fnv_saves_a() {
    let labels = actor_value_enum().map_or_else(Vec::new, |enum_def| {
        (0..enum_def.get_name_count())
            .map(|index| enum_def.get_name_of(i64::from(index)))
            .collect()
    });
    WB_ACTOR_VALUE_LABELS.set(labels);
}

/// Port of `DefineFNVSaves`.
pub fn define_fnv_saves() {
    set_file_magic("FO3SAVEGAME");
    set_extract_info(&EXTRACT_INFO_SAVE);
    set_file_plugins("Plugins");
    define_fnv();
    define_fnv_saves_a();
    define_fnv_saves_s();
}

/// Port of `SwitchToFNVCoSave`.
pub fn switch_to_fnv_co_save() {
    set_file_magic("NVSE");
    set_extract_info(&[]);
    set_file_plugins("Absolute:44");
    set_file_chapters(WB_CO_SAVE_CHAPTERS.get());
    set_file_header(WB_CO_SAVE_HEADER.get());
}
