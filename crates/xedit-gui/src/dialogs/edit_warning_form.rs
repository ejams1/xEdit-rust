// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeEditWarningForm.pas (FormCreate, FormShow,
// Timer1Timer) and xEdit/xeEditWarningForm.dfm (Panel1/2/3/4, Image1/2,
// Label1, LabelMessage, btnOk, BitBtn1, Timer1) and
// xEdit/xeMainForm.pas (EditWarn, EditWarnOk)

//! `TfrmEditWarning`: the "Time to think..." warning before the first edit.
//!
//! The OK button counts down from three seconds (`Timer1`, one second), and
//! every tenth use the countdown is shorter (`EditWarnCount div 10` from the
//! `[Usage]` section). The shell reads `[Usage] EditWarnCount` through
//! `settings.get`, shows the form, and on OK writes the incremented count
//! through `settings.set`, as `EditWarn` does.
//!
//! `EditWarn` itself (`TfrmMain.EditWarn`) skips the form once OK was given
//! in the run, with `DebugHook <> 0`, and with `wbIKnowWhatImDoing`; the
//! shell keeps that flag.

use std::time::{Duration, Instant};

use eframe::egui;

/// The message of `LabelMessage` (`xeEditWarningForm.dfm`).
pub const WARNING_MESSAGE: &str = "You are about to edit a module file.\r\rIf you have the slightest doubt about\rwhat the implications of your change\rare, you shouldn't be doing it.\r\rAre you absolutely sure you want to continue?";

/// What the dialog asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditWarningAction {
    /// `btnOk` (`mrOk`): the edit continues and `EditWarnCount` grows.
    Ok,
    /// `BitBtn1` (`mrAbort`): the edit is refused.
    Cancelled,
}

/// `TfrmEditWarning` as data.
#[derive(Clone, Debug, PartialEq)]
pub struct EditWarningState {
    /// `TimerCount`: `3` minus `EditWarnCount div 10`, one tick a second.
    pub timer_count: i32,
    /// `[Usage] EditWarnCount` as the form read it.
    pub edit_warn_count: i32,
    /// `Timer1.Interval`.
    pub interval: Duration,
    /// The tick clock of `Timer1`.
    pub last_tick: Instant,
}

/// `FormCreate` with the `EditWarnCount` of the `[Usage]` section.
pub fn edit_warning_state(edit_warn_count: i32) -> EditWarningState {
    let mut state = EditWarningState {
        timer_count: 3,
        edit_warn_count,
        interval: Duration::from_secs(1),
        last_tick: Instant::now(),
    };
    // `TimerCount := TimerCount - (EditWarnCount div 10)`.
    state.timer_count -= edit_warn_count / 10;
    state
}

impl EditWarningState {
    /// `FormShow`: the OK button is enabled once the countdown is done.
    pub fn ok_enabled(&self) -> bool {
        self.timer_count <= 0
    }

    /// `btnOk.Caption`: `&Yes I'm absolutely sure`, with the count while the
    /// timer runs.
    pub fn ok_caption(&self) -> String {
        if self.ok_enabled() {
            "Yes I'm absolutely sure".to_owned()
        } else {
            format!("Yes I'm absolutely sure ({})", self.timer_count)
        }
    }

    /// `Timer1Timer`: `Dec(TimerCount)` then `FormShow`.
    pub fn timer_tick(&mut self) {
        self.timer_count -= 1;
    }

    /// Whether the timer's interval passed since the last tick, and the
    /// tick itself (`Timer1.Enabled` while the countdown runs).
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.timer_count <= 0 {
            return false;
        }
        if now.duration_since(self.last_tick) >= self.interval {
            self.last_tick = now;
            self.timer_tick();
            true
        } else {
            false
        }
    }

    /// `EditWarn`'s write: `Inc(EditWarnCount)` after an OK.
    pub fn incremented_count(&self) -> i32 {
        self.edit_warn_count + 1
    }
}

/// Draws the warning (`BorderStyle = bsNone`, `fsStayOnTop`,
/// `poScreenCenter` in the DFM; the port keeps a centred window with the
/// two buttons).
pub fn show(ctx: &egui::Context, state: &mut EditWarningState) -> Option<EditWarningAction> {
    let mut action = None;
    egui::Window::new("Time to think...")
        .id(egui::Id::unique("frmEditWarning"))
        .collapsible(false)
        .resizable(false)
        .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(556.0, 291.0)))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Warning!");
                ui.add_space(8.0);
                for line in WARNING_MESSAGE.split("\r") {
                    ui.label(line);
                }
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(state.ok_enabled(), egui::Button::new(state.ok_caption()))
                    .clicked()
                {
                    action = Some(EditWarningAction::Ok);
                }
                if ui.button("Hmm.. let me think about it some more").clicked() {
                    action = Some(EditWarningAction::Cancelled);
                }
            });
        });
    if action.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        action = Some(EditWarningAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_countdown_starts_at_three_seconds() {
        let mut state = edit_warning_state(0);
        assert_eq!(state.timer_count, 3);
        assert!(!state.ok_enabled());
        assert_eq!(state.ok_caption(), "Yes I'm absolutely sure (3)");
        state.timer_tick();
        state.timer_tick();
        state.timer_tick();
        assert!(state.ok_enabled());
        assert_eq!(state.ok_caption(), "Yes I'm absolutely sure");
    }

    #[test]
    fn every_tenth_use_shortens_the_countdown() {
        // `TimerCount := 3 - (EditWarnCount div 10)`.
        assert_eq!(edit_warning_state(9).timer_count, 3);
        assert_eq!(edit_warning_state(10).timer_count, 2);
        assert_eq!(edit_warning_state(30).timer_count, 0);
        assert!(edit_warning_state(30).ok_enabled());
        assert_eq!(edit_warning_state(100).timer_count, -7);
    }

    #[test]
    fn a_tick_waits_for_the_interval() {
        let mut state = edit_warning_state(0);
        let start = state.last_tick;
        assert!(!state.tick(start));
        assert!(!state.tick(start + Duration::from_millis(500)));
        assert!(state.tick(start + Duration::from_secs(1)));
        assert_eq!(state.timer_count, 2);
        // Once the countdown is done the timer stops (`Timer1.Enabled :=
        // False`).
        state.timer_count = 0;
        assert!(!state.tick(start + Duration::from_secs(5)));
    }

    #[test]
    fn an_ok_increments_the_usage_count() {
        let state = edit_warning_state(7);
        assert_eq!(state.incremented_count(), 8);
    }
}
