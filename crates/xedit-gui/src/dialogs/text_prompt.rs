// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (the `InputQuery` calls of
// mniNavChangeFormIDClick, mniNavChangeReferencingRecordsClick,
// mniNavRenumberFormIDsFromClick, mniNavCopyIdleClick,
// mniNavCreateMergedPatchClick, mniNavCreateDeltaPatchClick,
// mniCreateNewFileClick and CopyInto)

//! `InputQuery`: the message box with a caption, a text and an OK and a
//! Cancel button. Upstream's `InputQuery` is the VCL's `InputQuery` with
//! the `New FormID` and the `New Module File` prompts of the main form;
//! the port draws the same box.
//!
//! UPSTREAM-QUIRK: the VCL's `InputQuery` is a modal message box with an
//! edit field, and the port draws a window with the same fields, the same
//! buttons and the same OK/Cancel answer.

use eframe::egui;

use crate::dialogs::selection::DialogPlacement;

/// What the box answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextPromptAction {
    /// `mrOk`: the text of the box.
    Ok(String),
    /// `mrCancel`.
    Cancelled,
}

/// `InputQuery` as data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextPromptState {
    /// The caption of the box (`InputQuery`'s first argument).
    pub caption: String,
    /// The message of the box (`InputQuery`'s second argument).
    pub message: String,
    /// The text of the box, which opens with the value the caller suggests.
    pub value: String,
    /// The dialog refuses an empty answer when this is set; upstream's
    /// `AddNewFile` and the delta patch name box refuse it too.
    pub require_text: bool,
    /// The error of the last answer, shown under the box.
    pub error: Option<String>,
    pub placement: Option<DialogPlacement>,
    pub rect: Option<egui::Rect>,
}

impl TextPromptState {
    /// A box over a caption and a message, with the suggested text.
    pub fn new(caption: impl Into<String>, message: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            caption: caption.into(),
            message: message.into(),
            value: value.into(),
            require_text: false,
            error: None,
            placement: None,
            rect: None,
        }
    }
}

/// Draws the box.
pub fn show(ctx: &egui::Context, state: &mut TextPromptState) -> Option<TextPromptAction> {
    let mut action = None;
    let mut open = true;
    let mut focused = false;
    let mut window = egui::Window::new(if state.caption.is_empty() {
        "xEdit"
    } else {
        &state.caption
    })
    .id(egui::Id::unique("frmInputQuery"))
    .collapsible(false)
    .resizable(false)
    .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(460.0, 160.0)))
    .open(&mut open);
    window = match state.placement {
        Some(placement) => placement.apply(ctx, window),
        None => window.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)),
    };
    let response = window.show(ctx, |ui| {
        if !state.message.is_empty() {
            ui.label(&state.message);
        }
        let edit = ui.add(egui::TextEdit::singleline(&mut state.value).desired_width(400.0));
        focused = edit.has_focus();
        if let Some(error) = &state.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                if state.require_text && state.value.trim().is_empty() {
                    state.error = Some("Please enter a value".to_owned());
                } else {
                    action = Some(TextPromptAction::Ok(state.value.clone()));
                }
            }
            if ui.button("Cancel").clicked() {
                action = Some(TextPromptAction::Cancelled);
            }
        });
    });
    if let Some(response) = response {
        state.rect = Some(response.response.rect);
    }
    // `FormKeyDown`: Enter answers the box, Escape cancels it.
    if action.is_none() {
        if ctx.input(|input| input.key_pressed(egui::Key::Enter)) && !focused {
            if state.require_text && state.value.trim().is_empty() {
                state.error = Some("Please enter a value".to_owned());
            } else {
                action = Some(TextPromptAction::Ok(state.value.clone()));
            }
        } else if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            action = Some(TextPromptAction::Cancelled);
        }
    }
    if !open && action.is_none() {
        action = Some(TextPromptAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_opens_with_the_suggested_text() {
        let state = TextPromptState::new("New FormID", "Please enter the new FormID in hex.", "04000000");
        assert_eq!(state.caption, "New FormID");
        assert_eq!(state.value, "04000000");
    }
}
