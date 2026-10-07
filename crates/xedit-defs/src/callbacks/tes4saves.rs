// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsTES4Saves.pas

//! The callbacks of `wbDefinitionsTES4Saves.pas` that are ported by hand.
//! The ones that are not ported yet are stubs in `tes4saves_stubs.rs`.

#[allow(unused_imports)]
pub use super::tes4saves_stubs::*;

use xedit_core::delphi::round;
use xedit_core::interface::globals::{
    set_extract_info, set_file_chapters, set_file_header, set_file_magic, set_file_plugins,
};
use xedit_core::interface::*;

use super::fo4saves::{self, native};
use super::save_interface::wb_find_save_element;
use crate::tes4::define_tes4;
use crate::tes4saves::{WB_ACTOR_VALUE_LABELS, WB_CO_SAVE_CHAPTERS, WB_CO_SAVE_HEADER, define_tes4_saves_s};

/// Upstream `ExtractInfoSave`: the chapters of a save that are initialized
/// while it loads.
const EXTRACT_INFO_SAVE: [u8; 2] = [3, 4];

/// Port of `DefineTES4SavesA`: the labels of the actor value arrays.
pub fn define_tes4_saves_a() {
    let labels = actor_value_enum().map_or_else(Vec::new, |enum_def| {
        (0..enum_def.get_name_count())
            .map(|index| enum_def.get_name_of(i64::from(index)))
            .collect()
    });
    WB_ACTOR_VALUE_LABELS.set(labels);
}

/// Port of `DefineTES4Saves`. The oracle reads Oblivion saves after it
/// warns that they are not supported yet; the magic is the one upstream
/// sets, `FO3SAVEGAME`, so no Oblivion save passes its check.
pub fn define_tes4_saves() {
    set_file_magic("FO3SAVEGAME");
    set_extract_info(&EXTRACT_INFO_SAVE);
    set_file_plugins("Plugins");
    define_tes4();
    define_tes4_saves_a();
    define_tes4_saves_s();
}

/// Port of `SwitchToTES4CoSave`.
pub fn switch_to_tes4_co_save() {
    set_file_magic("OBSE");
    set_extract_info(&[]);
    set_file_plugins("Absolute:44");
    set_file_chapters(WB_CO_SAVE_CHAPTERS.get());
    set_file_header(WB_CO_SAVE_HEADER.get());
}

/// Upstream `ScreenShotDataCounter`: three bytes per pixel.
pub fn screen_shot_data_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    super::fnvsaves::screen_shot_data_counter(a_base_ptr, a_element)
}

/// Upstream `OBSEChaptersDecider`: the member for the chunk type.
pub fn obse_chapters_decider(_a_base_ptr: DataPtr, a_element: ElementArg) -> i32 {
    let Some(element) = a_element else { return 0 };
    let chunk = wb_find_save_element("Chunk", element);
    let Some(kind) = chunk
        .as_data_container()
        .and_then(|chunk| chunk.get_element_by_name("Type"))
    else {
        return 0;
    };
    match kind.get_value().as_str() {
        "MODS" => 1,
        "STVS" => 2,
        "STVR" => 3,
        "STVE" => 4,
        "ARVS" => 5,
        "ARVR" => 6,
        "ARVE" => 7,
        "GLOB" => 8,
        _ => 9,
    }
}

/// Upstream `OBSEChapterGlobalCounter`: the globals of nine bytes in the
/// chunk, as the Variant division rounds to the count.
pub fn obse_chapter_global_counter(_a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    let Some(element) = a_element else { return 0 };
    let chunk = wb_find_save_element("Chunk", element);
    let Some(length) = chunk
        .as_data_container()
        .and_then(|chunk| chunk.get_element_by_name("Length"))
    else {
        return 0;
    };
    round(native(Some(length)) as f64 / 9.0) as u32
}

// ----- generated -----
/// Upstream `ChangedFormsCounter`, the same as for Fallout 4.
pub fn changed_forms_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::changed_forms_counter(a_base_ptr, a_element)
}

/// Upstream `GlobalData1Counter`, the same as for Fallout 4.
pub fn global_data1_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::global_data1_counter(a_base_ptr, a_element)
}

/// Upstream `GlobalData2Counter`, the same as for Fallout 4.
pub fn global_data2_counter(a_base_ptr: DataPtr, a_element: ElementArg) -> u32 {
    fo4saves::global_data2_counter(a_base_ptr, a_element)
}

/// Upstream `RefIDTableAfterLoad`, the same as for Fallout 4.
pub fn ref_id_table_after_load(a_element: &ElementRef) {
    fo4saves::ref_id_table_after_load(a_element)
}

/// Upstream `WorldspaceTableAfterLoad`, the same as for Fallout 4.
pub fn worldspace_table_after_load(a_element: &ElementRef) {
    fo4saves::worldspace_table_after_load(a_element)
}
