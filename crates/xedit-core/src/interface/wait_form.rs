// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas (IwbProgress, IwbWaitForm,
// TwbCreateWaitForm, wbCreateWaitForm, TwbNullWaitForm, TwbNullProgress,
// NullCreateWaitForm) and xEdit/xeWaitForm.pas (TfrmWait, TwbProgress,
// _CreateWaitForm), xEdit/xeMainForm.pas (GeneralProgress,
// GeneralProgressNoAbortCheck, btnCancelClick, UpdatePnlCancelVisible)

//! The wait form of a long operation: `IwbWaitForm` and `IwbProgress` of
//! `wbInterface.pas`, with the null implementations headless code runs
//! against.
//!
//! Upstream a long operation creates its wait form (`wbCreateWaitForm`),
//! asks it for a progress interface per stage (`CreateProgress`) and polls
//! `IsCanceled` at its loop points; the main form's cancel panel sets
//! `wbForceTerminate`, which `GeneralProgress` turns into an `Abort` at the
//! next progress message. The port keeps both shapes:
//!
//! - [`WaitForm`] and [`Progress`] are the interfaces, [`null_create_wait_form`]
//!   is `NullCreateWaitForm`, and [`set_create_wait_form`] installs the GUI's
//!   factory. Headless code never installs one, so [`wb_create_wait_form`]
//!   answers the null form and [`is_canceled`] is always `false`: the CLI
//!   runs exactly the operations it ran before.
//! - [`set_current_wait_form`] names the form of the operation the calling
//!   thread is inside (upstream the operation holds its own interface
//!   reference); [`poll_cancel`] is the `if wbForceTerminate then Abort`
//!   check of `GeneralProgress`, and [`general_progress`] is that whole
//!   procedure. The command loops of `xedit-analysis` and `xedit-session`
//!   call the poll at the places upstream polls, and a cancelled operation
//!   stops at the next one.

use std::sync::{Arc, RwLock};

use super::globals;

/// The progress interface of one stage of an operation: upstream
/// `IwbProgress`, whose only method is `UpdateStatus`.
pub trait Progress: Send + Sync {
    /// `UpdateStatus(aPosition, aStatus)`: set the stage's position, at most
    /// its maximum, and its status line.
    fn update_status(&self, position: i64, status: &str);
}

/// The wait form of a long operation: upstream `IwbWaitForm`.
pub trait WaitForm: Send + Sync {
    /// `GetIsCanceled`: whether Cancel was pressed and the form allows it.
    fn is_canceled(&self) -> bool;

    /// `CreateProgress`: a progress interface of a stage.
    fn create_progress(&self, caption: &str, status: &str, max: i64) -> Arc<dyn Progress>;
}

/// `TwbCreateWaitForm`: the factory of a wait form.
pub type CreateWaitForm = Arc<dyn Fn(&str, &str, bool, i32, i32) -> Arc<dyn WaitForm> + Send + Sync>;

/// `TwbNullProgress`: a progress interface that does nothing.
struct NullProgress;

impl Progress for NullProgress {
    fn update_status(&self, _position: i64, _status: &str) {}
}

/// `TwbNullWaitForm`: the wait form of a caller without a user.
struct NullWaitForm;

impl WaitForm for NullWaitForm {
    /// `TwbNullWaitForm.GetIsCanceled`: `False`.
    fn is_canceled(&self) -> bool {
        false
    }

    /// `TwbNullWaitForm.CreateProgress`: a `TwbNullProgress`.
    fn create_progress(&self, _caption: &str, _status: &str, _max: i64) -> Arc<dyn Progress> {
        Arc::new(NullProgress)
    }
}

/// `NullCreateWaitForm`: the factory headless code runs against.
pub fn null_create_wait_form(
    _caption: &str,
    _message: &str,
    _can_cancel: bool,
    _show_delay: i32,
    _update_delay: i32,
) -> Arc<dyn WaitForm> {
    Arc::new(NullWaitForm)
}

static CREATE_WAIT_FORM: RwLock<Option<CreateWaitForm>> = RwLock::new(None);
static CURRENT_WAIT_FORM: RwLock<Option<Arc<dyn WaitForm>>> = RwLock::new(None);

/// Installs upstream `wbCreateWaitForm`; `None` restores `NullCreateWaitForm`.
pub fn set_create_wait_form(factory: Option<CreateWaitForm>) {
    *CREATE_WAIT_FORM.write().unwrap() = factory;
}

/// Whether a factory is installed (the GUI's).
pub fn has_create_wait_form() -> bool {
    CREATE_WAIT_FORM.read().unwrap().is_some()
}

/// Port of `wbCreateWaitForm`: the installed factory's form, else the null
/// form.
pub fn wb_create_wait_form(
    caption: &str,
    message: &str,
    can_cancel: bool,
    show_delay: i32,
    update_delay: i32,
) -> Arc<dyn WaitForm> {
    let factory = CREATE_WAIT_FORM.read().unwrap().clone();
    match factory {
        Some(factory) => factory(caption, message, can_cancel, show_delay, update_delay),
        None => null_create_wait_form(caption, message, can_cancel, show_delay, update_delay),
    }
}

/// Names the wait form of the operation this thread is inside; `None` ends
/// it. The job runner sets the GUI's form around a command, as an operation
/// upstream holds its own `IwbWaitForm`.
pub fn set_current_wait_form(form: Option<Arc<dyn WaitForm>>) {
    *CURRENT_WAIT_FORM.write().unwrap() = form;
}

/// The wait form of the operation this thread is inside, if any.
pub fn current_wait_form() -> Option<Arc<dyn WaitForm>> {
    CURRENT_WAIT_FORM.read().unwrap().clone()
}

/// `GetIsCanceled` of the operation's wait form, or `wbForceTerminate` (the
/// main form's cancel panel, upstream `btnCancelClick`).
pub fn is_canceled() -> bool {
    if let Some(form) = current_wait_form()
        && form.is_canceled()
    {
        return true;
    }
    globals::force_terminate()
}

/// Why an operation stopped: upstream `Abort` (`EAbort`), which
/// `GeneralProgress` raises once `wbForceTerminate` is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the operation was cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// The `if wbForceTerminate then Abort` of `GeneralProgress`: `Err` once the
/// operation's wait form was cancelled or the main form's cancel panel was
/// pressed.
pub fn poll_cancel() -> Result<(), Cancelled> {
    if is_canceled() { Err(Cancelled) } else { Ok(()) }
}

/// Port of `GeneralProgressNoAbortCheck`: sends the line to the progress
/// callback.
pub fn general_progress_no_abort_check(status: &str) {
    super::misc::progress(status);
}

/// Port of `GeneralProgress`: the progress line, then the abort check.
pub fn general_progress(status: &str) -> Result<(), Cancelled> {
    general_progress_no_abort_check(status);
    poll_cancel()
}

/// Clears the installed factory and the operation form (tests, and a new
/// session).
pub fn reset() {
    set_create_wait_form(None);
    set_current_wait_form(None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

    use crate::interface::globals::test_lock;

    #[derive(Default)]
    struct TestState {
        canceled: AtomicBool,
        can_cancel: AtomicBool,
        position: AtomicI64,
        max: AtomicI64,
    }

    struct TestForm {
        state: Arc<TestState>,
    }

    impl TestForm {
        fn new(can_cancel: bool) -> Self {
            let state = Arc::new(TestState::default());
            state.can_cancel.store(can_cancel, Ordering::Relaxed);
            Self { state }
        }
    }

    struct TestProgress {
        state: Arc<TestState>,
    }

    impl Progress for TestProgress {
        fn update_status(&self, position: i64, _status: &str) {
            // `if aPosition > prgMax then aPosition := prgMax`.
            let max = self.state.max.load(Ordering::Relaxed);
            self.state.position.store(position.min(max), Ordering::Relaxed);
        }
    }

    impl WaitForm for TestForm {
        fn is_canceled(&self) -> bool {
            self.state.can_cancel.load(Ordering::Relaxed) && self.state.canceled.load(Ordering::Relaxed)
        }

        fn create_progress(&self, _caption: &str, _status: &str, max: i64) -> Arc<dyn Progress> {
            self.state.max.store(max, Ordering::Relaxed);
            Arc::new(TestProgress {
                state: Arc::clone(&self.state),
            })
        }
    }

    fn install(can_cancel: bool) -> Arc<TestForm> {
        let form = Arc::new(TestForm::new(can_cancel));
        set_current_wait_form(Some(form.clone()));
        form
    }

    #[test]
    fn the_null_form_never_cancels_and_keeps_no_position() {
        reset();
        let form = wb_create_wait_form("caption", "message", true, 0, 0);
        assert!(!form.is_canceled());
        let progress = form.create_progress("stage", "starting", 10);
        progress.update_status(5, "half way");
        assert!(!has_create_wait_form());
        reset();
    }

    #[test]
    fn an_installed_factory_is_used_by_wb_create_wait_form() {
        reset();
        set_create_wait_form(Some(Arc::new(|_caption, _message, can_cancel, _show, _update| {
            Arc::new(TestForm::new(can_cancel)) as Arc<dyn WaitForm>
        })));
        let form = wb_create_wait_form("caption", "message", true, 0, 0);
        assert!(has_create_wait_form());
        assert!(!form.is_canceled());
        reset();
    }

    #[test]
    fn a_cancelled_operation_stops_at_the_next_poll_point() {
        let _guard = test_lock();
        reset();
        let form = install(true);
        assert!(general_progress("stage 1").is_ok());
        assert!(poll_cancel().is_ok());
        form.state.canceled.store(true, Ordering::Relaxed);
        // A cancelled form stops the operation at the next poll.
        assert_eq!(poll_cancel(), Err(Cancelled));
        assert_eq!(general_progress("stage 2"), Err(Cancelled));
        reset();
    }

    #[test]
    fn a_form_that_can_not_cancel_ignores_its_flag() {
        let _guard = test_lock();
        reset();
        let form = install(false);
        form.state.canceled.store(true, Ordering::Relaxed);
        // `GetIsCanceled` is `fwCanCancel and fwCanceled`.
        assert!(!is_canceled());
        assert!(poll_cancel().is_ok());
        reset();
    }

    #[test]
    fn the_force_terminate_flag_of_the_main_form_cancels() {
        let _guard = test_lock();
        reset();
        let previous = globals::force_terminate();
        globals::set_force_terminate(true);
        assert!(is_canceled());
        assert_eq!(poll_cancel(), Err(Cancelled));
        globals::set_force_terminate(previous);
        reset();
    }

    #[test]
    fn a_progress_of_the_form_clamps_its_position_to_the_maximum() {
        let _guard = test_lock();
        reset();
        let form = Arc::new(TestForm::new(true));
        set_current_wait_form(Some(form.clone()));
        let progress = form.create_progress("stage", "starting", 3);
        progress.update_status(5, "over the maximum");
        assert_eq!(form.state.position.load(Ordering::Relaxed), 3);
        reset();
    }
}
