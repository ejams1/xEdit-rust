// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeRichEditForm.pas (FormCreate, FormShow,
// FormClose, FormKeyDown, edSearchKeyPress, btnTOCClick, BuildTOC,
// TOCSelected, tbrZoomChange, splTOCMoved, tmrEnableButtonTimer,
// vstTOCCollapsing, vstTOCGetText, vstTOCNodeDblClick) and
// xEdit/xeRichEditForm.dfm (edSearch, tbrZoom, btnTOC, pnlTOC with vstTOC,
// splTOC, pnlMain with reMain, btnOk, cbDontShowAgain, tmrEnableButton)

//! `TfrmRichEdit`: the help and message viewer: the RTF document, the
//! table of contents, the zoom and the search, with the OK button the
//! 500 ms timer enables and the once-only check box. The document is parsed
//! by `dialogs::viewers`; the TOC, zoom and width are the `[frmRichEdit]`
//! settings.

use std::time::{Duration, Instant};

use eframe::egui;

use crate::dialogs::viewers::{RtfDocument, RtfViewer, RtfViewerState};

/// `fcWhatsNew` of `xeMainForm.dfm`: the "What's New?" document.
pub const WHATS_NEW_RTF: &[u8] = include_bytes!("../../resources/whats_new.rtf");

/// `tmrEnableButton.Interval`.
pub const ENABLE_DELAY: Duration = Duration::from_millis(500);

/// What the dialog asks the shell to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RichEditAction {
    /// `btnOk`: the check box decides whether the version is recorded.
    Ok { dont_show_again: bool },
    /// The window was closed while `btnOk` was enabled (`FormClose` is
    /// `caNone` before that).
    Closed,
    /// The settings the viewer wrote while it was open (`TOC`,
    /// `TOCWidth`, `Zoom`).
    Settings { toc: bool, toc_width: i32, zoom: i32 },
}

/// `TfrmRichEdit`'s state.
#[derive(Clone, Debug)]
pub struct RichEditState {
    /// `Caption` ("What's New?", "Help", ...).
    pub title: String,
    pub document: RtfDocument,
    pub viewer: RtfViewerState,
    /// `btnOk.Visible`.
    pub ok_visible: bool,
    /// `btnOk.Enabled`, enabled by the timer.
    pub ok_enabled: bool,
    /// When `tmrEnableButton` was started (`FormShow`).
    pub shown_at: Instant,
    /// `cbDontShowAgain.Visible`.
    pub dont_show_again_visible: bool,
    /// The settings last written, so the shell writes only what changed.
    pub settings_written: Option<(bool, i32, i32)>,
    /// `Window` placement: `poMainFormCenter` has no stored rectangle.
    pub closing: bool,
}

impl RichEditState {
    /// Opens the viewer on a document with the `[frmRichEdit]` settings.
    pub fn new(title: impl Into<String>, document: RtfDocument, toc: bool, toc_width: i32, zoom: i32) -> Self {
        let viewer = RtfViewerState {
            toc_visible: toc,
            toc_width: toc_width as f32,
            zoom: zoom.clamp(25, 400),
            ..RtfViewerState::default()
        };
        Self {
            title: title.into(),
            document,
            viewer,
            ok_visible: true,
            ok_enabled: false,
            shown_at: Instant::now(),
            dont_show_again_visible: true,
            settings_written: None,
            closing: false,
        }
    }

    /// `FormShow`'s `tmrEnableButton.Enabled := True` and
    /// `tmrEnableButtonTimer`.
    pub fn tick(&mut self) {
        if !self.ok_enabled && self.shown_at.elapsed() >= ENABLE_DELAY {
            self.ok_enabled = true;
        }
    }

    /// `btnTOCClick`, `tbrZoomChange` and `splTOCMoved`: the settings the
    /// form writes as they change.
    pub fn settings(&self) -> (bool, i32, i32) {
        (
            self.viewer.toc_visible,
            self.viewer.toc_width.round() as i32,
            self.viewer.zoom,
        )
    }

    /// The settings to write this frame, once.
    pub fn take_settings(&mut self) -> Option<(bool, i32, i32)> {
        let settings = self.settings();
        if self.settings_written == Some(settings) {
            return None;
        }
        self.settings_written = Some(settings);
        Some(settings)
    }
}

/// Draws the viewer. The caller runs `settings.set` for a
/// [`RichEditAction::Settings`] and records `[WhatsNew] Version` on
/// `Ok { dont_show_again: true }`.
pub fn show(ctx: &egui::Context, state: &mut RichEditState) -> Option<RichEditAction> {
    state.tick();
    ctx.request_repaint_after(Duration::from_millis(50));
    // `FormKeyDown`: Enter and Escape click OK (`btnOk.Click`).
    let key_ok = ctx.input(|input| input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Escape))
        && !ctx.egui_wants_keyboard_input();
    let mut action = None;
    let mut open = true;
    egui::Window::new(state.title.clone())
        .id(egui::Id::unique("frmRichEdit"))
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(860.0, 620.0))
        .default_pos(egui::pos2(60.0, 60.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let ok_visible = state.ok_visible;
            let ok_enabled = state.ok_enabled;
            let dont_show_again_visible = state.dont_show_again_visible;
            let mut viewer = RtfViewer {
                id: "reMain",
                document: &state.document,
                state: &mut state.viewer,
                ok_visible,
                ok_enabled,
                dont_show_again_visible,
                dont_show_again_caption: "&Don't show again until changed",
            };
            let output = viewer.show(ui);
            if output.ok_clicked {
                action = Some(RichEditAction::Ok {
                    dont_show_again: state.viewer.dont_show_again,
                });
            }
        });
    if let Some(settings) = state.take_settings() {
        // The shell records `[frmRichEdit] TOC`, `TOCWidth` and `Zoom`.
        action = action.or(Some(RichEditAction::Settings {
            toc: settings.0,
            toc_width: settings.1,
            zoom: settings.2,
        }));
    }
    if key_ok && state.ok_enabled && action.is_none() {
        action = Some(RichEditAction::Ok {
            dont_show_again: state.viewer.dont_show_again,
        });
    }
    // `FormClose`: the window does not close while `btnOk` is disabled.
    if !open {
        if state.ok_enabled {
            action = action.or(Some(RichEditAction::Closed));
        } else {
            // `Action := caNone`.
        }
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_start_from_the_ini_and_the_ok_button_waits_for_the_timer() {
        let document = RtfDocument::default();
        let mut state = RichEditState::new("What's New?", document, true, 220, 130);
        assert!(state.viewer.toc_visible);
        assert_eq!(state.viewer.toc_width, 220.0);
        assert_eq!(state.viewer.zoom, 130);
        assert!(!state.ok_enabled);
        state.tick();
        assert!(!state.ok_enabled);
        state.shown_at = Instant::now() - ENABLE_DELAY;
        state.tick();
        assert!(state.ok_enabled);
    }

    #[test]
    fn the_settings_are_written_once_per_change() {
        let mut state = RichEditState::new("Help", RtfDocument::default(), false, 185, 100);
        let first = state.take_settings();
        assert_eq!(first, Some((false, 185, 100)));
        assert_eq!(state.take_settings(), None);
        state.viewer.zoom = 150;
        assert_eq!(state.take_settings(), Some((false, 185, 150)));
    }

    #[test]
    fn a_zoom_outside_the_track_bar_is_clamped() {
        let state = RichEditState::new("Help", RtfDocument::default(), false, 185, 900);
        assert_eq!(state.viewer.zoom, 400);
        let state = RichEditState::new("Help", RtfDocument::default(), false, 185, 1);
        assert_eq!(state.viewer.zoom, 25);
    }
}
