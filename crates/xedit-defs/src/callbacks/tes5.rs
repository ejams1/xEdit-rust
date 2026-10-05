// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDefinitionsTES5.pas

//! The callbacks of `wbDefinitionsTES5.pas` that are ported by hand. The
//! ones that are not ported yet are stubs in `tes5_stubs.rs`.

pub use super::tes5_stubs::*;

use xedit_core::interface::*;

/// Upstream `CombineVarRecs`.
pub fn combine_var_recs(a: &[VarRec], b: &[VarRec]) -> Vec<VarRec> {
    a.iter().chain(b).cloned().collect()
}

/// Upstream `MakeVarRecs`.
pub fn make_var_recs(a: &[VarRec]) -> Vec<VarRec> {
    a.to_vec()
}

/// Upstream `CmpW32`: -1, 0 or 1 for two unsigned values.
pub fn cmp_w32(a: u32, b: u32) -> i32 {
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}
