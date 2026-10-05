// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! Port of `wbInterface.pas`, split into submodules that follow the order of
//! the upstream unit.

pub mod array;
pub mod byte_array;
pub mod def;
pub mod element;
pub mod enum_def;
pub mod flags;
pub mod float;
pub mod form_id;
pub mod form_id_formater;
pub mod formaters;
pub mod globals;
pub mod guid;
pub mod integer;
pub mod len_string;
pub mod main_record;
pub mod misc;
pub mod resolvable;
pub mod string;
pub mod struct_def;
pub mod sub_record;
pub mod sub_record_group;
pub mod types;

pub use array::*;
pub use byte_array::*;
pub use def::*;
pub use element::*;
pub use enum_def::*;
pub use flags::*;
pub use float::*;
pub use form_id::{CRC32, FileID, FormID, ModuleType, ObjectIDOutOfBounds, SlotCounts};
pub use form_id_formater::*;
pub use formaters::*;
pub use globals::{GameMode, ToolMode, ToolSource};
pub use guid::*;
pub use integer::*;
pub use len_string::*;
pub use main_record::*;
pub use misc::Variant;
pub use resolvable::*;
pub use string::*;
pub use struct_def::{StructDef, StructDefArgs, StructSizeCallback};
pub use sub_record::*;
pub use sub_record_group::*;
pub use types::*;
