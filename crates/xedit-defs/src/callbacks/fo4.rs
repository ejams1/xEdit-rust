// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsFO4.pas

//! The callbacks of `wbDefinitionsFO4.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `fo4_stubs.rs`.

pub use super::fo4_stubs::*;

use std::sync::Arc;

use xedit_core::interface::*;

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
