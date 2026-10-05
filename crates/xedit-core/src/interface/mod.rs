// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! Port of `wbInterface.pas`, split into submodules that follow the order of
//! the upstream unit.

pub mod def;
pub mod element;
pub mod enum_def;
pub mod flags;
pub mod form_id;
pub mod formaters;
pub mod globals;
pub mod integer;
pub mod misc;
pub mod types;

pub use def::*;
pub use element::{Container, DataPtr, Element, ElementArg, ElementRef};
pub use enum_def::*;
pub use flags::*;
pub use form_id::{CRC32, FileID, FormID, ModuleType, ObjectIDOutOfBounds, SlotCounts};
pub use formaters::*;
pub use globals::{GameMode, ToolMode, ToolSource};
pub use integer::*;
pub use misc::Variant;
pub use types::*;
