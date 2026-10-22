// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The widgets every form of the GUI is built from: the virtualised tree
//! and grid, their conflict colour roles, and the theme and font helpers
//! the dialogs share (`xeApplyFontAndScale`).
//!
//! Nothing in this module reimplements a form: `main_form.rs` and the
//! modules of `dialogs/` draw through it.

pub mod conflict;
pub mod grid;
pub mod push_like_button;
pub mod virtual_tree;

use eframe::egui;

use crate::theme::ThemeChoice;

/// Port of `xeApplyFontAndScale` (`xeMainForm.pas`): applies the font of
/// `[UI] FontRecords` to the controls of a form. In egui the font is a
/// property of the context and the scale follows it, so this only has to
/// apply the theme's font to every widget style; the dialog modules call
/// it once at the top of their drawing.
pub fn apply_font_and_scale(ctx: &egui::Context, fonts: &crate::theme::Fonts) {
    crate::theme::apply_fonts(ctx, fonts);
}

/// Port of `TVclStylesSystemMenu` (Jvcl `Vcl.Styles.Utils.SystemMenu`), the
/// "Theme" entry the game select form and nine dialogs add to their system
/// menu: a menu with one radio entry per available style.
///
/// The port offers the three themes it can render; the currently chosen one
/// is marked, and a choice is written back to `UI.Theme` by the caller.
pub fn vcl_styles_system_menu(ui: &mut egui::Ui, current: &ThemeChoice) -> Option<ThemeChoice> {
    let mut chosen = None;
    ui.menu_button("Theme", |ui| {
        for (choice, label) in [
            (ThemeChoice::System, "System"),
            (ThemeChoice::Light, "Light"),
            (ThemeChoice::Dark, "Dark"),
        ] {
            let selected = *current == choice;
            if ui.radio(selected, label).clicked() {
                chosen = Some(choice);
            }
        }
        if matches!(current, ThemeChoice::Named(name) if !name.is_empty()) {
            ui.separator();
            ui.label("A VCL style name the port cannot render");
        }
    });
    chosen
}

/// The view that emitted a menu action, so a shared item (`mniNavOptions`
/// appears in two popups) resolves to the right handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuOrigin {
    TopBar,
    NavTree,
    NavTreeHeader,
    ViewTree,
    ViewHeader,
    ReferencedBy,
    Messages,
    Spreadsheet,
    ButtonStrip,
}

/// What a menu item asks the form to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    /// Run a session command (`xedit-session`'s registry name).
    Session(&'static str),
    /// A session command with a parameter. The run-time items of a menu
    /// name what they act on: the language of
    /// `mniMainLocalizationLanguage` is the language to switch to.
    SessionWithParam {
        /// The session command (`localization.language`).
        command: &'static str,
        /// The parameter of the command (the language to switch to).
        param: String,
    },
    /// A presentation-only item the form handles itself (a checkbox, a
    /// column-width mode, the messages memo).
    Presentation(&'static str),
    /// An item the port does not offer (the Pluggy Link integrations and
    /// CodeSite logging, which the ledger marks `excluded`).
    Excluded,
}

/// Resolves a clicked [`crate::menu_model::MenuItem`] into an action.
///
/// A covered binding with a command becomes [`MenuCommand::Session`]; a
/// presentation entry is handled by the form; an item the port does not
/// offer is drawn disabled and never reaches here.
pub fn menu_command(item: &crate::menu_model::MenuItem) -> MenuCommand {
    // `files.clean` is the command of four items with different passes; the
    // form has to see which item was clicked to send `itm` or `udr`.
    if item.command == Some("files.clean")
        && matches!(
            item.id,
            "mniNavUndeleteAndDisableReferences" | "mniNavRemoveIdenticalToMaster"
        )
    {
        MenuCommand::Presentation(item.id)
    } else if let Some(command) = item.command {
        MenuCommand::Session(command)
    } else if item.handler == Some("mniNavCleaningObsoleteClick") {
        // The obsolete cleaning entries are presentation: they explain
        // that cleaning moved to the new commands (CLEANING_OBSOLETE).
        MenuCommand::Presentation("cleaning_obsolete")
    } else if item.id.starts_with("mniMainPluggyLink") || item.id == "mniNavOtherCodeSiteLogging" {
        MenuCommand::Excluded
    } else {
        MenuCommand::Presentation(item.id)
    }
}
