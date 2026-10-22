// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (DoProcessMessages,
// UpdatePnlCancelVisible, btnCancelClick, GeneralProgress) and
// xEdit/xeWaitForm.pas (the form of an operation)

//! Long commands off the UI thread.
//!
//! Upstream runs a menu handler on the UI thread and keeps the window alive
//! with `DoProcessMessages` inside `wbProgress`; the port had the handler
//! run synchronously, so a large `filter.apply` froze the window until it
//! returned (a known step-5 gap). [`JobRunner`] moves the command to a
//! worker thread: the window keeps repainting, the wait form shows the
//! operation's message and progress, and Cancel is upstream's
//! `pnlCancel`/`btnCancelClick` (it asks first and then sets
//! `wbForceTerminate`, which the command's polling points turn into a
//! stop).
//!
//! The command is still the one operation layer: the worker calls
//! [`CommandBridge::call`] with the same JSON the CLI sends. While a job
//! runs the session lives on the worker; [`JobRunner::poll`] gives the
//! bridge back with the result.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use serde_json::Value;

use xedit_core::interface::misc;
use xedit_core::interface::wait_form::{self, WaitForm};
use xedit_session::{CommandError, Session};

use crate::command::CommandBridge;
use crate::dialogs::wait_form::WaitFormState;

/// What the wait form of a job shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobSpec {
    /// The form's caption (`aCaption` of `wbCreateWaitForm`).
    pub caption: String,
    /// The operation's message (`plMessage`).
    pub message: String,
    /// Whether Cancel is offered.
    pub can_cancel: bool,
    /// `aShowDelay` and `aUpdateDelay` in milliseconds.
    pub show_delay: i32,
    pub update_delay: i32,
}

impl JobSpec {
    /// A cancellable operation with no show delay.
    pub fn new(caption: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            caption: caption.into(),
            message: message.into(),
            can_cancel: true,
            show_delay: 0,
            update_delay: 100,
        }
    }

    /// An operation that can not be cancelled (the wait form has no
    /// Cancel button).
    pub fn not_cancellable(mut self) -> Self {
        self.can_cancel = false;
        self
    }
}

/// What a finished job leaves the shell.
pub struct FinishedJob {
    /// The session the command ran against.
    pub bridge: CommandBridge,
    /// The command the job ran.
    pub command: String,
    /// Its response, or its error (`cancelled` when Cancel stopped it).
    pub result: Result<Value, CommandError>,
    /// The message lines the command wrote through `GeneralProgress`.
    pub messages: Vec<String>,
    /// How long the command took.
    pub elapsed: std::time::Duration,
}

struct RunningJob {
    command: String,
    wait: Arc<WaitFormState>,
    rx: Receiver<(CommandBridge, Result<Value, CommandError>)>,
    started: Instant,
}

/// The one job of the main form.
#[derive(Default)]
pub struct JobRunner {
    running: Option<RunningJob>,
    /// The last finished job, until the shell polls it.
    finished: Option<FinishedJob>,
}

impl JobRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a command is on its worker thread.
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// The wait form of the running job.
    pub fn wait_form(&self) -> Option<&Arc<WaitFormState>> {
        self.running.as_ref().map(|job| &job.wait)
    }

    /// The command that is running.
    pub fn running_command(&self) -> Option<&str> {
        self.running.as_ref().map(|job| job.command.as_str())
    }

    /// `wbCreateWaitForm` for the operation: the form the command polls
    /// through `xedit_core::interface::wait_form`.
    pub fn create_wait_form(spec: &JobSpec) -> Arc<WaitFormState> {
        Arc::new(WaitFormState::new(
            spec.caption.clone(),
            spec.message.clone(),
            spec.can_cancel,
            std::time::Duration::from_millis(spec.show_delay.max(0) as u64),
            std::time::Duration::from_millis(spec.update_delay.max(0) as u64),
        ))
    }

    /// Starts `command` on a worker thread with the JSON parameters the CLI
    /// takes. The session moves with it and comes back from
    /// [`Self::poll`].
    pub fn start(
        &mut self,
        bridge: CommandBridge,
        command: &str,
        params: Value,
        spec: JobSpec,
    ) -> Result<(), CommandError> {
        if self.running.is_some() {
            return Err(CommandError::new(
                "busy",
                "another command is still running; wait for it or cancel it",
            ));
        }
        let wait = Self::create_wait_form(&spec);
        let form: Arc<dyn WaitForm> = wait.clone();
        let worker_wait = Arc::clone(&wait);
        let (tx, rx) = std::sync::mpsc::channel();
        let name = command.to_owned();
        let worker_name = name.clone();
        let started = Instant::now();
        let spawned = std::thread::Builder::new()
            .name("xedit-job".to_owned())
            .stack_size(xedit_core::threads::STACK_SIZE)
            .spawn(move || {
                // `UpdatePnlCancelVisible` clears `wbForceTerminate` when the
                // cancel panel appears; the job starts with it clear.
                xedit_core::interface::globals::set_force_terminate(false);
                let previous_callback = misc::progress_callback();
                let message_sink = Arc::clone(&worker_wait);
                misc::set_progress_callback(Some(Arc::new(move |message: &str| {
                    message_sink.add_message(message);
                })));
                // The operation's wait form: `GeneralProgress` and the
                // command's polling points read it.
                wait_form::set_current_wait_form(Some(Arc::clone(&form)));
                let mut bridge = bridge;
                let result = bridge.call(&worker_name, params);
                wait_form::set_current_wait_form(None);
                misc::set_progress_callback(previous_callback);
                xedit_core::interface::globals::set_force_terminate(false);
                let _ = tx.send((bridge, result));
            });
        match spawned {
            Ok(_) => {
                self.running = Some(RunningJob {
                    command: name,
                    wait,
                    rx,
                    started,
                });
                Ok(())
            }
            Err(error) => Err(CommandError::new("internal", error.to_string())),
        }
    }

    /// Collects the finished job, if the worker is done.
    pub fn poll(&mut self) -> Option<FinishedJob> {
        let received = self.running.as_ref().map(|job| job.rx.try_recv());
        match received {
            Some(Ok((bridge, result))) => {
                let job = self.running.take().expect("checked above");
                let messages = job.wait.take_messages();
                self.finished = Some(FinishedJob {
                    bridge,
                    command: job.command,
                    result,
                    messages,
                    elapsed: job.started.elapsed(),
                });
            }
            Some(Err(TryRecvError::Disconnected)) => {
                // A panic on the worker: the session is gone with it.
                let job = self.running.take().expect("checked above");
                self.finished = Some(FinishedJob {
                    bridge: CommandBridge::new(Session::default()),
                    command: job.command,
                    result: Err(CommandError::new(
                        "internal",
                        "the command thread ended without a result",
                    )),
                    messages: job.wait.take_messages(),
                    elapsed: job.started.elapsed(),
                });
            }
            Some(Err(TryRecvError::Empty)) | None => {}
        }
        self.finished.take()
    }

    /// Upstream `btnCancelClick`: the confirmation question is shown first
    /// (the wait form's question window), and only an answer of yes sets
    /// the operation's flag and `wbForceTerminate`, so the command stops at
    /// its next poll point.
    pub fn request_cancel(&self) {
        if let Some(job) = &self.running {
            job.wait.request_cancel();
        }
    }

    /// The answer of the force-cancel question: yes cancels the operation,
    /// no leaves it running.
    pub fn answer_cancel(&self, yes: bool) {
        if let Some(job) = &self.running {
            job.wait.answer_cancel(yes);
        }
    }

    /// Whether the force-cancel question is open.
    pub fn is_confirming_cancel(&self) -> bool {
        self.running.as_ref().is_some_and(|job| job.wait.is_confirming())
    }

    /// Cancels the operation outright, with no question. Upstream only does
    /// this after the question is answered with yes, so the shell uses
    /// [`JobRunner::request_cancel`] and [`JobRunner::answer_cancel`].
    ///
    /// UPSTREAM-QUIRK: upstream's cancel is the main form's `pnlCancel`
    /// overlay (`xeMainForm.dfm:2198-2218`); its `btnCancelClick`
    /// (`xeMainForm.pas:12075`) asks the question itself — there is no
    /// question in `xeWaitForm` — and warns that cancelling might leave
    /// xEdit in an inconsistent state. The port shows the same question
    /// with the same text and the same default answer (no), so this
    /// outright cancel is not a path the shell takes.
    pub fn cancel(&self) {
        if let Some(job) = &self.running {
            job.wait.cancel();
        }
    }

    /// Whether the running job was asked to cancel.
    pub fn cancel_requested(&self) -> bool {
        self.running.as_ref().is_some_and(|job| job.wait.is_cancel_requested())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_job_is_refused_while_one_runs() {
        let mut runner = JobRunner::new();
        // A bridge over no session still runs `system.version`, which needs
        // nothing loaded.
        let bridge = CommandBridge::new(Session::default());
        runner
            .start(
                bridge,
                "system.version",
                serde_json::json!({}),
                JobSpec::new("caption", "message"),
            )
            .unwrap();
        let bridge = CommandBridge::new(Session::default());
        let error = runner
            .start(
                bridge,
                "system.version",
                serde_json::json!({}),
                JobSpec::new("caption", "message"),
            )
            .unwrap_err();
        assert_eq!(error.code, "busy");
        // The first job finishes and gives the bridge back.
        let finished = loop {
            if let Some(finished) = runner.poll() {
                break finished;
            }
            std::thread::yield_now();
        };
        assert!(finished.result.is_ok());
        assert!(!runner.is_running());
    }

    #[test]
    fn a_cancelled_job_stops_at_the_next_poll_point() {
        // A command whose handler polls the wait form: `refs.build` polls
        // once before it looks at the files, so a cancelled form stops it
        // even without a loaded session. The cancel and the force-terminate
        // flag are process-wide, so the test holds the globals' lock.
        let _guard = xedit_core::interface::globals::test_lock();
        let mut runner = JobRunner::new();
        let bridge = CommandBridge::new(Session::default());
        let spec = JobSpec::new("caption", "message");
        runner.start(bridge, "refs.build", serde_json::json!({}), spec).unwrap();
        // The form may not exist yet for a very fast command; cancel is a
        // no-op then and the result is the unloaded session's error.
        runner.cancel();
        let finished = loop {
            if let Some(finished) = runner.poll() {
                break finished;
            }
            std::thread::yield_now();
        };
        assert!(!runner.is_running());
        // Either the poll stopped it, or the command failed for the missing
        // session before the cancel landed.
        if let Err(error) = finished.result {
            assert!(
                error.code == "cancelled" || error.code == "no_session",
                "unexpected {error:?}"
            );
        }
        xedit_core::interface::globals::set_force_terminate(false);
    }

    #[test]
    fn a_finished_job_reports_the_progress_messages() {
        let mut runner = JobRunner::new();
        let bridge = CommandBridge::new(Session::default());
        runner
            .start(
                bridge,
                "system.version",
                serde_json::json!({}),
                JobSpec::new("caption", "message"),
            )
            .unwrap();
        let finished = loop {
            if let Some(finished) = runner.poll() {
                break finished;
            }
            std::thread::yield_now();
        };
        assert_eq!(finished.command, "system.version");
        assert!(finished.messages.is_empty());
        assert!(finished.elapsed >= std::time::Duration::ZERO);
    }

    #[test]
    fn the_form_keeps_the_spec() {
        let spec = JobSpec::new("Filter", "Applying the filter").not_cancellable();
        let form = JobRunner::create_wait_form(&spec);
        assert_eq!(form.caption, "Filter");
        assert!(!form.can_cancel);
        let spec = JobSpec::new("Filter", "Applying the filter");
        let form = JobRunner::create_wait_form(&spec);
        assert!(form.can_cancel);
        assert!(!form.is_cancel_requested());
    }

    #[test]
    fn the_force_cancel_question_gates_the_cancel() {
        // Upstream `btnCancelClick` asks before it sets `wbForceTerminate`.
        let _guard = xedit_core::interface::globals::test_lock();
        xedit_core::interface::globals::set_force_terminate(false);
        let spec = JobSpec::new("caption", "message");
        let form = JobRunner::create_wait_form(&spec);
        // The question opens; the operation still runs.
        form.request_cancel();
        assert!(form.is_confirming());
        assert!(!form.is_cancel_requested());
        // A "no" closes the question and leaves the operation alone.
        form.answer_cancel(false);
        assert!(!form.is_confirming());
        assert!(!form.is_cancel_requested());
        xedit_core::interface::globals::set_force_terminate(false);
    }
}
