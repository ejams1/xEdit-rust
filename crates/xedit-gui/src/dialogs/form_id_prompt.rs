// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (GetFormIDCallback: the
// `InputQuery('New FormID', 'Please enter the new FormID in hex. e.g.
// 0404CC43. The FormID needs to be a load order corrected form ID.', s)`
// and `TwbFormID.FromStr`)

//! The "New FormID" input box of `wbGetFormIDCallback`.
//!
//! Upstream asks with `InputQuery`; the port draws the same question, opens
//! the box with the suggested load order FormID
//! (`FileFormIDtoLoadOrderFormID(NewFormID, True)`), and hands the text to
//! `TwbFormID.FromStr`. A text that is not a FormID keeps the box open with
//! the exception's message, as `Application.HandleException` reports it
//! upstream; Cancel is the null FormID, which refuses the add.

use eframe::egui;

use super::prompts::parse_form_id;

/// The prompt of `InputQuery`.
pub const FORM_ID_PROMPT: &str =
    "Please enter the new FormID in hex. e.g. 0404CC43. The FormID needs to be a load order corrected form ID.";

/// What the prompt asks the seam.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormIdAction {
    /// OK: the text as the user typed it (`TwbFormID.FromStr` of it, or the
    /// null FormID for an empty box).
    Ok(String),
    /// Cancel: the null FormID.
    Cancelled,
}

/// The input box as data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormIdState {
    /// `InputQuery`'s default answer.
    pub text: String,
    /// The `EConvertError` text of a refused answer.
    pub error: Option<String>,
}

impl FormIdState {
    /// The box `GetFormIDCallback` opens.
    pub fn new(suggested: impl Into<String>) -> Self {
        Self {
            text: suggested.into(),
            error: None,
        }
    }

    /// The answer of the box: `TwbFormID.FromStr(s)`, which keeps the low
    /// 32 bits of `StrToInt64('$' + s)`; a text that does not convert keeps
    /// the box open.
    pub fn commit(&mut self) -> Option<FormIdAction> {
        if parse_form_id(&self.text).is_some() {
            self.error = None;
            Some(FormIdAction::Ok(self.text.clone()))
        } else {
            self.error = Some(format!("'{}' is not a valid FormID", self.text));
            None
        }
    }
}

/// Draws the prompt; `None` keeps it open.
pub fn show(ctx: &egui::Context, state: &mut FormIdState) -> Option<FormIdAction> {
    let mut action = None;
    egui::Window::new("New FormID")
        .id(egui::Id::unique("form-id-prompt"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(FORM_ID_PROMPT);
            let response = ui.add(egui::TextEdit::singleline(&mut state.text).desired_width(200.0));
            if let Some(error) = &state.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    action = state.commit();
                }
                if ui.button("Cancel").clicked() {
                    action = Some(FormIdAction::Cancelled);
                }
            });
        });
    if action.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        action = Some(FormIdAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use xedit_core::interface::FormID;

    #[test]
    fn the_box_opens_with_the_suggested_form_id() {
        let state = FormIdState::new("0404CC43");
        assert_eq!(state.text, "0404CC43");
        assert!(state.error.is_none());
    }

    #[test]
    fn ok_answers_the_text_that_parses() {
        let mut state = FormIdState::new("0404CC43");
        assert_eq!(state.commit(), Some(FormIdAction::Ok("0404CC43".to_owned())));
        // An empty box is the null FormID (the add is refused), which is
        // `TwbFormID.FromStr('')`.
        state.text = String::new();
        assert_eq!(parse_form_id(&state.text), Some(FormID::null()));
        assert!(state.commit().is_some());
    }

    #[test]
    fn a_text_that_is_not_a_form_id_keeps_the_box_open() {
        let mut state = FormIdState::new("zzzz");
        assert_eq!(state.commit(), None);
        assert!(state.error.is_some());
        state.text = "0404CC43".to_owned();
        assert!(state.commit().is_some());
        assert!(state.error.is_none());
    }
}
