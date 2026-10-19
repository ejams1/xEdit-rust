// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The xEdit GUI: the main window of `xeMainForm.pas`, the dialogs of
//! `xEdit.dpr`'s 21 forms, and the widgets they draw with.
//!
//! The GUI is one more front end of the command registry in
//! `xedit-session` (design rule 1): it runs the same requests the CLI, the
//! daemon and the MCP server run, and adds no command of its own. The
//! initial load is the CLI's startup step (`Session::load`); the startup
//! dialogs gather the game and the module selection that the CLI takes from
//! its command line.
//!
//! Modules follow the upstream forms one to one: `main_form.rs` is
//! `xeMainForm.pas`, `dialogs/game_select_form.rs` is `xeGameSelectForm.pas`
//! and so on. Everything is drawn through `widgets/`, which owns the
//! virtualised tree and grid, the conflict colours and the theme.

pub mod command;
pub mod dialogs;
pub mod icon;
pub mod main_form;
pub mod menu_model;
pub mod settings;
pub mod startup;
pub mod theme;
pub mod widgets;

pub use main_form::TfrmMain;

/// The load hands its session to the UI thread, so the session must be
/// `Send`; this asserts it at compile time rather than at the first load.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<xedit_session::Session>();
};
