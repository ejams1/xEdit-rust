// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The analyses of xEdit over loaded plugins: conflict detection (the
//! conflict code of `xeMainForm.pas`, moved out of the GUI) and the array
//! alignment it uses (`TDiff`).

pub mod cleaning;
pub mod conflict;
pub mod diff;
pub mod filter;
