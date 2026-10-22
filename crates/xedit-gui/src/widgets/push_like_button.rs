// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xePushLikeButton.pas (TButton with FPushLike,
// SetPushLike, Toggle, CNCommand, SetButtonStyle, CreateParams, CreateWnd,
// GetChecked, SetChecked, TPushLikeButtonStyleHook.DrawButton)

//! The checkbox-style push button: `xePushLikeButton.pas` replaces
//! `Vcl.StdCtrls.TButton` with a `BS_PUSHLIKE or BS_CHECKBOX` variant of it,
//! which stays pressed while `Checked` is on. `btnTOC` of
//! `xeRichEditForm.pas` is the only user.
//!
//! The port keeps the state machine without the window handle:
//!
//! - a click (`CN_COMMAND` with `BN_CLICKED`) of a push-like button toggles
//!   `Checked` and does **not** run the `OnClick` handler (upstream
//!   `CNCommand` swallows the message then);
//! - `set_checked` sets the value and then, when the button is push-like and
//!   its clicks are not disabled, raises the `OnClick` handler (`Click`),
//!   which the port reports as a [`PushLikeEvent::Clicked`].
//!
//! The style hook paints a checked push-like button pressed; in egui that is
//! the selected state of [`push_like_button`].

use eframe::egui;

/// What a click of the button did, so the caller can run the `OnClick`
/// handler it would have run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushLikeEvent {
    /// Nothing happened (the click of a plain button is reported as
    /// [`PushLikeEvent::Clicked`] by [`push_like_button`]).
    None,
    /// `CNCommand` toggled `Checked`; the `OnClick` handler does not run.
    Toggled,
    /// `SetChecked` changed the value and `Click` raised `OnClick`.
    Clicked,
}

/// The state of one push-like button.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushLikeButton {
    /// `FChecked`.
    pub checked: bool,
    /// `FPushLike`.
    pub push_like: bool,
    /// `ClicksDisabled`: `SetChecked` changes the value without clicking.
    pub clicks_disabled: bool,
    /// The caption of the button.
    pub caption: String,
}

impl PushLikeButton {
    pub fn new(caption: impl Into<String>) -> Self {
        Self {
            checked: false,
            push_like: false,
            clicks_disabled: false,
            caption: caption.into(),
        }
    }

    /// `SetPushLike`: turning the property on or off recreates the window
    /// upstream; the port only flips the flag.
    pub fn set_push_like(&mut self, value: bool) {
        self.push_like = value;
    }

    /// `CNCommand` with `BN_CLICKED`: a push-like button toggles and reports
    /// [`PushLikeEvent::Toggled`] (the `OnClick` handler is not run), a plain
    /// button reports [`PushLikeEvent::Clicked`].
    pub fn click(&mut self) -> PushLikeEvent {
        if self.push_like {
            // `Toggle`: `Checked := not FChecked`.
            self.checked = !self.checked;
            PushLikeEvent::Toggled
        } else {
            PushLikeEvent::Clicked
        }
    }

    /// `SetChecked`: the value changes, then a push-like button raises its
    /// `OnClick` handler unless its clicks are disabled.
    pub fn set_checked(&mut self, value: bool) -> PushLikeEvent {
        if self.checked == value {
            return PushLikeEvent::None;
        }
        self.checked = value;
        if self.push_like && !self.clicks_disabled {
            PushLikeEvent::Clicked
        } else {
            PushLikeEvent::None
        }
    }

    /// `GetChecked`.
    pub fn checked(&self) -> bool {
        self.checked
    }
}

/// Draws the button; the response reports a click, and the state machine is
/// advanced with [`PushLikeButton::click`] (the caller runs its `OnClick`
/// handler for [`PushLikeEvent::Clicked`]).
pub fn push_like_button(ui: &mut egui::Ui, state: &mut PushLikeButton) -> egui::Response {
    // `TPushLikeButtonStyleHook.DrawButton`: a checked push-like button
    // paints pressed.
    let pressed = state.push_like && state.checked;
    let response = ui.add(egui::Button::new(&state.caption).selected(pressed));
    if response.clicked() {
        let _ = state.click();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_push_like_click_toggles_without_the_click_handler() {
        let mut button = PushLikeButton::new("TOC");
        button.set_push_like(true);
        assert!(!button.checked());
        // `CNCommand`: `if FPushLike and (NotifyCode = BN_CLICKED) then
        // Toggle`, and the inherited handler (OnClick) is skipped.
        assert_eq!(button.click(), PushLikeEvent::Toggled);
        assert!(button.checked());
        assert_eq!(button.click(), PushLikeEvent::Toggled);
        assert!(!button.checked());
    }

    #[test]
    fn a_plain_button_click_only_raises_the_click_handler() {
        let mut button = PushLikeButton::new("OK");
        assert_eq!(button.click(), PushLikeEvent::Clicked);
        assert!(!button.checked());
        // `SetChecked` of a plain button changes the value silently.
        assert_eq!(button.set_checked(true), PushLikeEvent::None);
        assert!(button.checked());
    }

    #[test]
    fn set_checked_of_a_push_like_button_raises_click_only_on_a_change() {
        let mut button = PushLikeButton::new("TOC");
        button.set_push_like(true);
        assert_eq!(button.set_checked(true), PushLikeEvent::Clicked);
        assert!(button.checked());
        // `if FChecked <> Value then`: no change, no click.
        assert_eq!(button.set_checked(true), PushLikeEvent::None);
        assert_eq!(button.set_checked(false), PushLikeEvent::Clicked);
        assert!(!button.checked());
    }

    #[test]
    fn disabled_clicks_keep_set_checked_silent() {
        let mut button = PushLikeButton::new("TOC");
        button.set_push_like(true);
        button.clicks_disabled = true;
        assert_eq!(button.set_checked(true), PushLikeEvent::None);
        assert!(button.checked());
        // `CNCommand` still toggles when the user clicks.
        assert_eq!(button.click(), PushLikeEvent::Toggled);
        assert!(!button.checked());
    }
}
