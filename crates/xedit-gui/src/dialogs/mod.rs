// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The dialogs of the GUI, one module per upstream form (`xeGameSelectForm`
//! → `game_select_form.rs`, `xeModuleSelectForm` → `module_select_form.rs`).
//!
//! Step 1 of phase 7 brings the two dialogs the startup flow needs. Step 5
//! finishes them (the presets of the module selection, the file select
//! form) and adds the rest of the 21 forms. A dialog module draws through
//! the widgets of `crate::widgets` and returns an action; it never calls a
//! session command itself.

pub mod game_select_form;
pub mod module_select_form;
