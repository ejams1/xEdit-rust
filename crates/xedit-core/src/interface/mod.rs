// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! Port of `wbInterface.pas`, split into submodules that follow the order of
//! the upstream unit.

pub mod form_id;
pub mod globals;

pub use form_id::{CRC32, FileID, FormID, ModuleType, ObjectIDOutOfBounds, SlotCounts};
pub use globals::{GameMode, ToolMode, ToolSource};
