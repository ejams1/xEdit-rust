// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbDataFormatNifTypes.pas

//! The value types of NIF files: strings, vectors, colours, quaternions,
//! matrices and the many enumerations and flags. The builders are
//! generated into `data_format_nif_types/defs.rs` by
//! `cargo xtask port-defs emit-df`; the text callbacks are ported by hand
//! in `data_format_nif_types/callbacks.rs`.

mod callbacks;
mod defs;
mod stubs;

use std::sync::atomic::{AtomicBool, Ordering};

pub use callbacks::*;
pub use defs::*;
#[allow(unused_imports)]
pub use stubs::*;

#[allow(unused_imports)]
use crate::data_format::{
    DataType, Def, DefKind, DfError, El, Event, OnDecide, R, Tree, df_array, df_bool, df_bytes, df_chars, df_enum,
    df_flags, df_float, df_hex_integer, df_integer, df_merge, df_struct, df_union, df_value_union, req, same_text,
};
#[allow(unused_imports)]
use crate::variant::Variant;

/// `wbRotationEuler`: rotations print as Euler angles (yaw, pitch, roll in
/// degrees) instead of an angle and an axis.
pub static ROTATION_EULER: AtomicBool = AtomicBool::new(false);

pub fn rotation_euler() -> bool {
    ROTATION_EULER.load(Ordering::Relaxed)
}
