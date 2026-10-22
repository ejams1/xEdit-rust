// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeWaitForm.pas (TfrmWait: bnCancelClick,
// FormCreate, CreateProgress, fwDoUpdate, GetIsCanceled, _AddRef/_Release;
// TwbProgress: Create, UpdateStatus, prgAddToWait/prgRemoveFromWait;
// _CreateWaitForm) and xEdit/xeWaitForm.dfm (plMessage, Panel2, plCancel
// with bnCancel). The confirmation question is the main form's cancel
// overlay, xEdit/xeMainForm.pas (pnlCancel, btnCancelClick).

//! `TfrmWait`: the wait form of a long operation, as the progress seam of
//! `xedit_core::interface::wait_form`.
//!
//! The form shows the operation's message, the status of each progress
//! stage, and (when the operation allows it) a Cancel button. The port
//! shows the form after `show_delay` and repaints on `update_delay`, as
//! `fwDoUpdate` does.
//!
//! UPSTREAM-QUIRK: upstream never installs its wait form
//! (`xeWaitForm.pas:247` leaves `wbCreateWaitForm := _CreateWaitForm`
//! commented out and `Core/wbInterface.pas:5082` keeps
//! `wbCreateWaitForm := NullCreateWaitForm`), so upstream's wait form
//! never reaches the screen and its `fwCanceled` never becomes true. The
//! only cancel upstream offers is the main form's `pnlCancel` overlay
//! (`xeMainForm.dfm:2198-2218`), whose `btnCancelClick`
//! (`xeMainForm.pas:12075`) asks first and then sets `wbForceTerminate`.
//! The port's single Cancel button merges the two upstream semantics: it
//! asks the question of `pnlCancel` and, on a yes, sets `fwCanceled` and
//! `wbForceTerminate` together.
//!
//! The state is shared with the worker thread through an `Arc`; the
//! progress list and message list are behind mutexes, the cancel flag is an
//! atomic.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use xedit_core::interface::wait_form::{Progress, WaitForm};

/// One `TwbProgress` of the form: its caption, status line and position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressEntry {
    pub caption: String,
    pub status: String,
    pub position: i64,
    pub max: i64,
}

/// What the form asks the shell to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitFormAction {
    /// The Cancel button (`bnCancel` in upstream's wait form, `btnCancel`
    /// in the main form's cancel overlay).
    Cancel,
}

/// Upstream `btnCancelClick`'s question (`xeMainForm.pas:12075`): the warning
/// the user sees before the operation is forced to stop.
pub const FORCE_CANCEL_QUESTION: &str = "Are you sure you want to request to force cancel the current operation?\r\n\r\nCanceling the operation might leave xEdit in an inconsistent state.";

/// `TfrmWait` as data, shared with the worker thread of the operation.
pub struct WaitFormState {
    /// `Caption` of the form.
    pub caption: String,
    /// `plMessage.Caption`.
    pub message: String,
    /// `fwCanCancel`: `plCancel.Visible` follows it.
    pub can_cancel: bool,
    /// `fwShowDelay` and `fwUpdateDelay` in milliseconds.
    pub show_delay: Duration,
    pub update_delay: Duration,
    /// `fwCreated`.
    pub created: Instant,
    /// `fwCanceled`.
    canceled: AtomicBool,
    /// Whether the force-cancel question is open (upstream's `btnCancelClick`
    /// shows the `MessageDlg` before it sets `wbForceTerminate`).
    confirming: AtomicBool,
    /// Whether the form was shown, `Visible` upstream.
    shown: AtomicBool,
    /// The progress stages in the order they were created (`fwProgressHead`
    /// to `fwProgressTail`).
    progress: Arc<Mutex<Vec<ProgressEntry>>>,
    /// The message log lines the operation sent while it ran
    /// (`GeneralProgress`'s `AddMessage`).
    messages: Arc<Mutex<Vec<String>>>,
}

impl WaitFormState {
    /// `TfrmWait.Create(caption, message, canCancel, showDelay, updateDelay)`.
    pub fn new(
        caption: impl Into<String>,
        message: impl Into<String>,
        can_cancel: bool,
        show_delay: Duration,
        update_delay: Duration,
    ) -> Self {
        Self {
            caption: caption.into(),
            message: message.into(),
            can_cancel,
            show_delay,
            update_delay,
            created: Instant::now(),
            canceled: AtomicBool::new(false),
            confirming: AtomicBool::new(false),
            shown: AtomicBool::new(false),
            progress: Arc::new(Mutex::new(Vec::new())),
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// `fwCreated` plus `fwShowDelay` is when the form appears.
    pub fn should_show(&self) -> bool {
        self.created.elapsed() >= self.show_delay
    }

    /// `fwDoUpdate`: whether the update delay has passed, which arms the
    /// show and the repaint.
    pub fn update_due(&self, last_update: &mut Instant) -> bool {
        if last_update.elapsed() >= self.update_delay {
            *last_update = Instant::now();
            true
        } else {
            false
        }
    }

    /// Upstream's `bnCancelClick` (`xeWaitForm.pas:107`): `fwCanceled :=
    /// True`. The port sets `wbForceTerminate` as well, because the port's
    /// Cancel is upstream's `pnlCancel` force-cancel (see the file header).
    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::Relaxed);
        xedit_core::interface::globals::set_force_terminate(true);
    }

    /// Upstream's `btnCancelClick` (`xeMainForm.pas:12075`) asks first: the
    /// question is [`FORCE_CANCEL_QUESTION`] and the default answer is no.
    /// The cancel itself only happens when the user answers yes, so the
    /// flag is not set before the question is answered.
    pub fn request_cancel(&self) {
        self.confirming.store(true, Ordering::Relaxed);
    }

    /// Whether the force-cancel question is open.
    pub fn is_confirming(&self) -> bool {
        self.confirming.load(Ordering::Relaxed)
    }

    /// The answer of the force-cancel question: yes cancels the operation,
    /// no leaves it running.
    pub fn answer_cancel(&self, yes: bool) {
        self.confirming.store(false, Ordering::Relaxed);
        if yes {
            self.cancel();
        }
    }

    /// The straight state of the Cancel button.
    pub fn is_cancel_requested(&self) -> bool {
        self.canceled.load(Ordering::Relaxed)
    }

    /// The progress stages, in creation order.
    pub fn stages(&self) -> Vec<ProgressEntry> {
        self.progress.lock().map(|stages| stages.clone()).unwrap_or_default()
    }

    /// `GeneralProgress`'s `AddMessage`: one line per CRLF part.
    pub fn add_message(&self, message: &str) {
        if let Ok(mut messages) = self.messages.lock() {
            for line in message.split("\r\n") {
                messages.push(line.to_owned());
            }
        }
    }

    /// The message lines the operation sent, drained.
    pub fn take_messages(&self) -> Vec<String> {
        self.messages
            .lock()
            .map(|mut messages| std::mem::take(&mut *messages))
            .unwrap_or_default()
    }

    /// `fwDoUpdate`'s `if not Visible then ... Show`.
    pub fn mark_shown(&self) {
        self.shown.store(true, Ordering::Relaxed);
    }

    pub fn is_shown(&self) -> bool {
        self.shown.load(Ordering::Relaxed)
    }
}

impl WaitForm for WaitFormState {
    /// `GetIsCanceled`: `Result := fwCanCancel and fwCanceled`.
    fn is_canceled(&self) -> bool {
        self.can_cancel && self.is_cancel_requested()
    }

    /// `CreateProgress`: a `TwbProgress` of a new stage.
    fn create_progress(&self, caption: &str, status: &str, max: i64) -> Arc<dyn Progress> {
        let mut stages = self.progress.lock().expect("the progress list");
        stages.push(ProgressEntry {
            caption: caption.to_owned(),
            status: status.to_owned(),
            position: 0,
            max,
        });
        Arc::new(ProgressHandle {
            stages: Arc::clone(&self.progress),
            index: stages.len() - 1,
        })
    }
}

/// `TwbProgress`: one stage of the form.
struct ProgressHandle {
    stages: Arc<Mutex<Vec<ProgressEntry>>>,
    index: usize,
}

impl Progress for ProgressHandle {
    /// `UpdateStatus`: `if aPosition > prgMax then aPosition := prgMax`,
    /// then the status line.
    fn update_status(&self, position: i64, status: &str) {
        if let Ok(mut stages) = self.stages.lock()
            && let Some(entry) = stages.get_mut(self.index)
        {
            entry.position = position.min(entry.max);
            entry.status = status.to_owned();
        }
    }
}

/// Draws the wait form; `Some(WaitFormAction::Cancel)` is a press of
/// `bnCancel`.
pub fn show(ctx: &egui::Context, form: &WaitFormState) -> Option<WaitFormAction> {
    if !form.should_show() {
        return None;
    }
    form.mark_shown();
    let mut action = None;
    egui::Window::new(&form.caption)
        .id(egui::Id::unique("frmWait"))
        .collapsible(false)
        .resizable(true)
        .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(541.0, 325.0)))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            // `plMessage`: the operation's message.
            ui.label(&form.message);
            ui.add_space(6.0);
            // The progress stages (`TwbProgress` list).
            egui::ScrollArea::vertical()
                .id_salt("wait-progress")
                .max_height(160.0)
                .show(ui, |ui| {
                    for stage in form.stages() {
                        ui.horizontal(|ui| {
                            ui.strong(&stage.caption);
                            if stage.max > 0 {
                                ui.add(
                                    egui::ProgressBar::new((stage.position as f32 / stage.max as f32).clamp(0.0, 1.0))
                                        .desired_width(160.0)
                                        .text(format!("{}/{}", stage.position, stage.max)),
                                );
                            }
                            ui.label(&stage.status);
                        });
                    }
                });
            ui.add_space(6.0);
            ui.label(format!("{:.1} s", form.created.elapsed().as_secs_f32()));
            if form.can_cancel {
                // `plCancel` with `bnCancel`.
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    if ui.button("Cancel").clicked() {
                        action = Some(WaitFormAction::Cancel);
                    }
                });
            }
        });
    action
}

/// Upstream's `btnCancelClick` question: a `MessageDlg` with Yes and No
/// and no as the default answer, drawn over the wait form.
pub fn show_cancel_question(ctx: &egui::Context, form: &WaitFormState) -> Option<bool> {
    if !form.is_confirming() {
        return None;
    }
    let mut answer = None;
    egui::Window::new("xEdit")
        .id(egui::Id::unique("frmForceCancel"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(FORCE_CANCEL_QUESTION);
            ui.horizontal(|ui| {
                if ui.button("Yes").clicked() {
                    answer = Some(true);
                }
                // No is the default answer upstream (`mbNo`).
                if ui.button("No").clicked() {
                    answer = Some(false);
                }
            });
        });
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_that_can_not_cancel_ignores_the_button() {
        // `cancel` sets the process-wide `wbForceTerminate`, so the test
        // holds the globals' lock.
        let _guard = xedit_core::interface::globals::test_lock();
        let form = WaitFormState::new("caption", "message", false, Duration::ZERO, Duration::ZERO);
        form.cancel();
        // `GetIsCanceled` is `fwCanCancel and fwCanceled`.
        assert!(!WaitForm::is_canceled(&form));
        assert!(form.is_cancel_requested());
        xedit_core::interface::globals::set_force_terminate(false);
    }

    #[test]
    fn the_stages_report_their_position_clamped_to_the_maximum() {
        let form = WaitFormState::new("caption", "message", true, Duration::ZERO, Duration::ZERO);
        let progress = form.create_progress("Building", "starting", 10);
        progress.update_status(4, "forty percent");
        progress.update_status(40, "over the maximum");
        let stages = form.stages();
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].caption, "Building");
        assert_eq!(stages[0].status, "over the maximum");
        assert_eq!(stages[0].position, 10);
    }

    #[test]
    fn a_cancelled_form_stops_the_operation_of_the_job_runner() {
        use xedit_core::interface::globals::test_lock;
        let _guard = test_lock();
        xedit_core::interface::wait_form::reset();
        xedit_core::interface::globals::set_force_terminate(false);
        let form = Arc::new(WaitFormState::new(
            "caption",
            "message",
            true,
            Duration::ZERO,
            Duration::ZERO,
        ));
        xedit_core::interface::wait_form::set_current_wait_form(Some(form.clone()));
        assert!(xedit_core::interface::wait_form::poll_cancel().is_ok());
        form.cancel();
        assert_eq!(
            xedit_core::interface::wait_form::poll_cancel(),
            Err(xedit_core::interface::wait_form::Cancelled)
        );
        xedit_core::interface::wait_form::reset();
        xedit_core::interface::globals::set_force_terminate(false);
    }

    #[test]
    fn messages_split_at_crlf_as_add_message_does() {
        let form = WaitFormState::new("caption", "message", true, Duration::ZERO, Duration::ZERO);
        form.add_message("first\r\nsecond");
        assert_eq!(form.take_messages(), vec!["first".to_owned(), "second".to_owned()]);
        assert!(form.take_messages().is_empty());
    }

    #[test]
    fn the_force_cancel_question_only_cancels_after_a_yes() {
        use xedit_core::interface::globals::test_lock;
        let _guard = test_lock();
        xedit_core::interface::globals::set_force_terminate(false);
        let form = WaitFormState::new("caption", "message", true, Duration::ZERO, Duration::ZERO);
        // Upstream's `btnCancelClick` asks before it sets `wbForceTerminate`.
        form.request_cancel();
        assert!(form.is_confirming());
        assert!(!form.is_cancel_requested());
        // A "no" leaves the operation running.
        form.answer_cancel(false);
        assert!(!form.is_confirming());
        assert!(!form.is_cancel_requested());
        // A "yes" stops it.
        form.request_cancel();
        form.answer_cancel(true);
        assert!(!form.is_confirming());
        assert!(form.is_cancel_requested());
        xedit_core::interface::globals::set_force_terminate(false);
    }

    #[test]
    fn the_form_waits_for_its_show_delay() {
        let form = WaitFormState::new("caption", "message", true, Duration::from_millis(2000), Duration::ZERO);
        assert!(!form.should_show());
        let form = WaitFormState::new("caption", "message", true, Duration::ZERO, Duration::ZERO);
        assert!(form.should_show());
    }
}
