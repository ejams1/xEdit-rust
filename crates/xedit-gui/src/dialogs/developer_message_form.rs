// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeDeveloperMessageForm.pas (FormCreate,
// FormShow, FormClose, FixZoom, btnOopsClick, btnOKClick, btnKoFiClick,
// btnPayPalClick, tmrEnableButtonTimer) and
// xEdit/xeDeveloperMessageForm.dfm (reMain with the fcMessage and
// fcMessageThanks documents, pnlElminster with the fcImage bitmap,
// pnlBottom with btnOops, cbDontShowAgain, btnCancel, btnOK, btnKoFi and
// btnPayPal, tmrEnableButton)

//! `TfrmDeveloperMessage`: the release's message to the user, its donation
//! links, the "Oops, I'm not a patron" button, the 5 s delay on Close and
//! the once-only check box. The documents and the bitmap are the release's
//! own resources (`resources/dev_message*.rtf`, `resources/dev_image.bmp`),
//! decompressed from the 4.1.5q DFMs, MPL 2.0.

use std::time::{Duration, Instant};

use eframe::egui;

use crate::dialogs::viewers::{RtfDocument, RtfViewer, RtfViewerState, parse_rtf};

/// `fcMessage` of the form.
pub const MESSAGE_RTF: &[u8] = include_bytes!("../../resources/dev_message.rtf");
/// `fcMessageThanks` of the form.
pub const THANKS_RTF: &[u8] = include_bytes!("../../resources/dev_message_thanks.rtf");
/// `fcImage`: the 200x200 bitmap next to the text.
pub const IMAGE_BMP: &[u8] = include_bytes!("../../resources/dev_image.bmp");

/// `tmrEnableButton.Interval`.
pub const ENABLE_DELAY: Duration = Duration::from_millis(5000);

/// `TdmChoice`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DmChoice {
    #[default]
    Patreon,
    KoFi,
    PayPal,
}

impl DmChoice {
    pub fn url(self) -> &'static str {
        match self {
            DmChoice::Patreon => "https://www.patreon.com/ElminsterAU",
            DmChoice::KoFi => "https://www.ko-fi.com/ElminsterAU",
            DmChoice::PayPal => "https://paypal.me/ElminsterAU",
        }
    }
}

/// What the dialog asks the shell to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeveloperMessageAction {
    /// The form closed: the choice, the check box and whether the patron
    /// flag was cleared (`btnOopsClick`). The shell writes
    /// `[DeveloperMessage] LastShownOn` and `Version`, and `[Options]
    /// Patron` when it was cleared.
    Closed {
        choice: DmChoice,
        dont_show_again: bool,
        patron_cleared: bool,
        /// A donation button closed the form: the shell opens the choice's
        /// URL (`ShowDeveloperMessage`'s `bnKoFi.Click` and the like).
        open_url: bool,
    },
    /// `tmrEnableButton` enabled Close.
    Timer,
}

/// `TfrmDeveloperMessage`'s state.
#[derive(Clone)]
pub struct DeveloperMessageState {
    /// `wbPatron`.
    pub patron: bool,
    pub message: RtfDocument,
    pub thanks: RtfDocument,
    /// `imgElminster.Picture`: decoded from the release's bitmap.
    pub image: Option<egui::ColorImage>,
    /// The decoded texture, kept across frames.
    pub texture: Option<egui::TextureHandle>,
    pub viewer: RtfViewerState,
    /// `btnCancel.Enabled` (`tmrEnableButtonTimer`).
    pub cancel_enabled: bool,
    pub shown_at: Instant,
    /// `btnOops.Visible` (the patron starts with it shown).
    pub oops_visible: bool,
    /// `Choice`: `dmcPatreon` from `btnOKClick`.
    pub choice: DmChoice,
    /// Whether the patron flag was cleared this session.
    pub patron_cleared: bool,
    /// The viewer's zoom (`FixZoom`).
    pub zoom: i32,
}

impl DeveloperMessageState {
    /// `FormCreate`: the patron text or the standard one, the bitmap and
    /// the fixed zoom.
    pub fn new(patron: bool) -> Self {
        let mut state = Self {
            patron,
            message: parse_rtf(MESSAGE_RTF),
            thanks: parse_rtf(THANKS_RTF),
            image: decode_bmp(IMAGE_BMP),
            texture: None,
            viewer: RtfViewerState::default(),
            cancel_enabled: false,
            shown_at: Instant::now(),
            oops_visible: patron,
            choice: DmChoice::Patreon,
            patron_cleared: false,
            zoom: 100,
        };
        state.fix_zoom();
        state
    }

    /// `FixZoom`: the release grows or shrinks the form until the text fits.
    /// egui lays the text out against the window, so the port keeps the
    /// zoom and lets the message scroll (UPSTREAM-QUIRK).
    pub fn fix_zoom(&mut self) {
        self.viewer.zoom = self.zoom;
    }

    /// The document the viewer shows: the thanks with the patron flag, else
    /// the message.
    pub fn document(&self) -> &RtfDocument {
        if self.patron { &self.thanks } else { &self.message }
    }

    /// `cbDontShowAgain.Caption`.
    pub fn dont_show_again_caption(&self) -> &'static str {
        if self.patron {
            "&Don't show again until changed"
        } else {
            "&Don't show again for a while"
        }
    }

    /// `tmrEnableButtonTimer`.
    pub fn tick(&mut self) -> Option<DeveloperMessageAction> {
        if !self.cancel_enabled && self.shown_at.elapsed() >= ENABLE_DELAY {
            self.cancel_enabled = true;
            return Some(DeveloperMessageAction::Timer);
        }
        None
    }

    /// `btnOopsClick`: the patron flag clears, the standard message loads
    /// and the donation buttons show.
    pub fn oops(&mut self) {
        self.patron = false;
        self.patron_cleared = true;
        self.oops_visible = false;
        self.fix_zoom();
    }

    /// The close action of a button (`btnOK`, `btnKoFi`, `btnPayPal`,
    /// `btnCancel`), with the check box state and whether the choice's URL
    /// is opened.
    pub fn closed(&self, choice: DmChoice, dont_show_again: bool, open_url: bool) -> DeveloperMessageAction {
        DeveloperMessageAction::Closed {
            choice,
            dont_show_again,
            patron_cleared: self.patron_cleared,
            open_url,
        }
    }
}

/// `imgElminster.Picture.LoadFromStream` of the release's BMP: a 24-bit
/// uncompressed bitmap, bottom-up.
pub fn decode_bmp(bytes: &[u8]) -> Option<egui::ColorImage> {
    if bytes.len() < 54 || &bytes[..2] != b"BM" {
        return None;
    }
    let offset = u32::from_le_bytes(bytes[10..14].try_into().ok()?) as usize;
    let width = i32::from_le_bytes(bytes[18..22].try_into().ok()?);
    let height = i32::from_le_bytes(bytes[22..26].try_into().ok()?);
    let bits = u16::from_le_bytes(bytes[28..30].try_into().ok()?);
    if bits != 24 || width <= 0 || height == 0 {
        return None;
    }
    let (width, height, top_down) = (width as usize, height.unsigned_abs() as usize, height < 0);
    let row_stride = (width * 3).div_ceil(4) * 4;
    let needed = offset.checked_add(row_stride.checked_mul(height)?)?;
    if bytes.len() < needed {
        return None;
    }
    let mut pixels = Vec::with_capacity(width * height);
    for row in 0..height {
        let source_row = if top_down { row } else { height - 1 - row };
        let start = offset + source_row * row_stride;
        for column in 0..width {
            let pixel = start + column * 3;
            pixels.push(egui::Color32::from_rgb(
                bytes[pixel + 2],
                bytes[pixel + 1],
                bytes[pixel],
            ));
        }
    }
    Some(egui::ColorImage::new([width, height], pixels))
}

/// Draws the message.
pub fn show(ctx: &egui::Context, state: &mut DeveloperMessageState) -> Option<DeveloperMessageAction> {
    let mut action = state.tick();
    ctx.request_repaint_after(Duration::from_millis(100));
    if state.texture.is_none()
        && let Some(image) = state.image.clone()
    {
        state.texture = Some(ctx.load_texture("Elminster", image, egui::TextureOptions::LINEAR));
    }
    let mut open = true;
    egui::Window::new("A message from the developer")
        .id(egui::Id::unique("frmDeveloperMessage"))
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(880.0, 620.0))
        .default_pos(egui::pos2(60.0, 60.0))
        .open(&mut open)
        .show(ctx, |ui| {
            let caption = state.dont_show_again_caption();
            let (oops_visible, patron, cancel_enabled) = (state.oops_visible, state.patron, state.cancel_enabled);
            let document = state.document().clone();
            let mut viewer = RtfViewer {
                id: "reMain",
                document: &document,
                state: &mut state.viewer,
                ok_visible: false,
                ok_enabled: true,
                dont_show_again_visible: true,
                dont_show_again_caption: caption,
            };
            let _ = viewer.show(ui);
            let dont_show_again = state.viewer.dont_show_again;
            // `pnlElminster`: the bitmap in the right of the text
            // (`FixZoom` sizes the square to the text height).
            if let Some(texture) = state.texture.clone() {
                let side = (ui.available_height() * 0.35).clamp(80.0, 200.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(side, side)));
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                let mut closed = None;
                if oops_visible && ui.button("Oops, I'm not a patron").clicked() {
                    state.oops();
                }
                if !patron {
                    if ui.button("Open Patreon").clicked() {
                        closed = Some((DmChoice::Patreon, true));
                    }
                    if ui.button("Ko-Fi").clicked() {
                        closed = Some((DmChoice::KoFi, true));
                    }
                    if ui.button("PayPal").clicked() {
                        closed = Some((DmChoice::PayPal, true));
                    }
                }
                if ui.add_enabled(cancel_enabled, egui::Button::new("Close")).clicked() {
                    closed = Some((DmChoice::Patreon, false));
                }
                if let Some((choice, open_url)) = closed {
                    state.choice = choice;
                    action = Some(state.closed(choice, dont_show_again, open_url));
                }
            });
        });
    // `FormClose`: `caNone` while Close is disabled and the result is not
    // `mrOk`.
    if !open && state.cancel_enabled {
        action = action.or(Some(state.closed(state.choice, state.viewer.dont_show_again, false)));
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_patron_sees_the_thanks_and_the_oops_button() {
        let mut state = DeveloperMessageState::new(true);
        assert_eq!(state.document().paragraphs.len(), state.thanks.paragraphs.len());
        assert_eq!(state.dont_show_again_caption(), "&Don't show again until changed");
        assert!(state.oops_visible);
        state.oops();
        assert!(!state.patron);
        assert!(state.patron_cleared);
        assert!(!state.oops_visible);
        assert_eq!(state.document().paragraphs.len(), state.message.paragraphs.len());
        assert_eq!(state.dont_show_again_caption(), "&Don't show again for a while");
    }

    #[test]
    fn the_standard_message_fits_in_the_form() {
        let state = DeveloperMessageState::new(false);
        assert!(!state.message.is_empty());
        assert!(!state.patron);
        assert!(!state.oops_visible);
    }

    #[test]
    fn the_close_button_waits_five_seconds() {
        let mut state = DeveloperMessageState::new(false);
        assert_eq!(state.tick(), None);
        assert!(!DeveloperMessageState::new(false).cancel_enabled);
        state.shown_at = Instant::now() - ENABLE_DELAY;
        assert_eq!(state.tick(), Some(DeveloperMessageAction::Timer));
        assert!(state.cancel_enabled);
        assert_eq!(state.tick(), None);
    }

    #[test]
    fn the_choice_carries_the_donation_url() {
        assert_eq!(DmChoice::Patreon.url(), "https://www.patreon.com/ElminsterAU");
        assert_eq!(DmChoice::KoFi.url(), "https://www.ko-fi.com/ElminsterAU");
        assert_eq!(DmChoice::PayPal.url(), "https://paypal.me/ElminsterAU");
        let state = DeveloperMessageState::new(false);
        assert_eq!(
            state.closed(DmChoice::KoFi, true, true),
            DeveloperMessageAction::Closed {
                choice: DmChoice::KoFi,
                dont_show_again: true,
                patron_cleared: false,
                open_url: true
            }
        );
    }

    #[test]
    fn the_bitmap_decodes_to_the_release_size() {
        let image = decode_bmp(IMAGE_BMP).unwrap();
        assert_eq!(image.size, [200, 200]);
        // A pixel in the top-left quadrant is not transparent.
        assert!(image.pixels[0].a() == 255);
        assert!(decode_bmp(b"not a bitmap").is_none());
    }
}
