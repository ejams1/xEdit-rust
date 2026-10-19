// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The dialogs of the GUI, one module per upstream form (`xeGameSelectForm`
//! → `game_select_form.rs`, `xeModuleSelectForm` → `module_select_form.rs`).
//!
//! Step 1 of phase 7 brought the two dialogs the startup flow needs. Step 5
//! finishes them and adds the six selection dialogs, which share
//! [`selection`]: the name filter with its regular expression mode, the
//! preset combo box, the check-list and selected-or-all rules, the
//! owner-drawn search, the confirmation questions and the placement values.
//! A dialog module draws through the widgets of `crate::widgets` and
//! returns an action; it never calls a session command itself.

pub mod file_select_form;
pub mod filter_options_form;
pub mod game_select_form;
pub mod mod_group_edit_form;
pub mod mod_group_select_form;
pub mod module_select_form;
pub mod selection;
