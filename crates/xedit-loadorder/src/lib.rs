// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Load order and session data of xEdit: the module information of the
//! data folder and the loaded files (the part of `wbLoadOrder.pas` the mod
//! groups read), the mod groups (`wbModGroups.pas`), and the Delphi text
//! and ini file behaviour their files are read and written with.

pub mod ini_files;
pub mod load_order;
pub mod mod_groups;
