// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsFO4.pas

//! The callbacks of `wbDefinitionsFO4.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `fo4_stubs.rs`.

pub use super::fo4_stubs::*;

use std::sync::Arc;

use xedit_core::interface::*;

use super::common::{collision_layer_links_to, index_key_from_ordinal};
use crate::common::{wb_idx_addon_node, wb_idx_collision_layer};

/// Upstream `CombineVarRecs`.
pub fn combine_var_recs(a: &[VarRec], b: &[VarRec]) -> Vec<VarRec> {
    a.iter().chain(b).cloned().collect()
}

/// Upstream `MakeVarRecs`.
pub fn make_var_recs(a: &[VarRec]) -> Vec<VarRec> {
    a.to_vec()
}

/// Upstream `GetObjectModPropertyEnum`. Not ported yet: it needs the
/// element tree.
pub fn get_object_mod_property_enum(_a_element: Option<Arc<dyn Element>>) -> Option<Arc<EnumDef>> {
    todo!("port GetObjectModPropertyEnum from wbDefinitionsFO4.pas line 2392")
}

/// Upstream `CmpW32`: -1, 0 or 1 for two unsigned values.
pub fn cmp_w32(a: u32, b: u32) -> i32 {
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Upstream `wbEdgeLinksTo0`.
pub fn wb_edge_links_to0(a_element: ElementArg) -> Option<ElementRef> {
    super::common::wb_edge_links_to(0, a_element)
}

/// Upstream `wbEdgeToStr0`.
pub fn wb_edge_to_str0(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    super::common::wb_edge_to_str(0, a_int, a_element, a_type)
}

/// Upstream `wbEdgeToInt0`.
pub fn wb_edge_to_int0(a_string: &str, a_element: ElementArg) -> i64 {
    super::common::wb_edge_to_int(0, a_string, a_element)
}

/// Upstream `wbVertexToStr0`.
pub fn wb_vertex_to_str0(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    super::common::wb_vertex_to_str(0, a_int, a_element, a_type)
}

/// Upstream `wbVertexToInt0`.
pub fn wb_vertex_to_int0(a_string: &str, a_element: ElementArg) -> i64 {
    super::common::wb_vertex_to_int(0, a_string, a_element)
}

/// Upstream `wbEdgeLinksTo1`.
pub fn wb_edge_links_to1(a_element: ElementArg) -> Option<ElementRef> {
    super::common::wb_edge_links_to(1, a_element)
}

/// Upstream `wbEdgeToStr1`.
pub fn wb_edge_to_str1(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    super::common::wb_edge_to_str(1, a_int, a_element, a_type)
}

/// Upstream `wbEdgeToInt1`.
pub fn wb_edge_to_int1(a_string: &str, a_element: ElementArg) -> i64 {
    super::common::wb_edge_to_int(1, a_string, a_element)
}

/// Upstream `wbVertexToStr1`.
pub fn wb_vertex_to_str1(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    super::common::wb_vertex_to_str(1, a_int, a_element, a_type)
}

/// Upstream `wbVertexToInt1`.
pub fn wb_vertex_to_int1(a_string: &str, a_element: ElementArg) -> i64 {
    super::common::wb_vertex_to_int(1, a_string, a_element)
}

/// Upstream `wbEdgeLinksTo2`.
pub fn wb_edge_links_to2(a_element: ElementArg) -> Option<ElementRef> {
    super::common::wb_edge_links_to(2, a_element)
}

/// Upstream `wbEdgeToStr2`.
pub fn wb_edge_to_str2(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    super::common::wb_edge_to_str(2, a_int, a_element, a_type)
}

/// Upstream `wbEdgeToInt2`.
pub fn wb_edge_to_int2(a_string: &str, a_element: ElementArg) -> i64 {
    super::common::wb_edge_to_int(2, a_string, a_element)
}

/// Upstream `wbVertexToStr2`.
pub fn wb_vertex_to_str2(a_int: i64, a_element: ElementArg, a_type: CallbackType) -> String {
    super::common::wb_vertex_to_str(2, a_int, a_element, a_type)
}

/// Upstream `wbVertexToInt2`.
pub fn wb_vertex_to_int2(a_string: &str, a_element: ElementArg) -> i64 {
    super::common::wb_vertex_to_int(2, a_string, a_element)
}

/// Upstream anonymous routine at line 7895 of `wbDefinitionsFO4.pas`: the
/// `ADDN` index key.
pub fn define_fo4_anonymous_7895(a_main_record: &MainRecordRef, a_index_keys: &mut IndexKeys) {
    index_key_from_ordinal(a_main_record, a_index_keys, "DATA", wb_idx_addon_node());
}

/// Upstream anonymous routine at line 9638 of `wbDefinitionsFO4.pas`: the
/// `COLL` index key.
pub fn define_fo4_anonymous_9638(a_main_record: &MainRecordRef, a_index_keys: &mut IndexKeys) {
    index_key_from_ordinal(a_main_record, a_index_keys, "BNAM", wb_idx_collision_layer());
}

/// Upstream anonymous routine at line 11711 of `wbDefinitionsFO4.pas`: the
/// collision layer of `XTRI`.
pub fn define_fo4_anonymous_11711(a_element: ElementArg) -> Option<ElementRef> {
    collision_layer_links_to(a_element)
}
