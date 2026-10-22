// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeWorldspaceCellDetailsForm.pas
// (xeGetCellDetailsForWorldspaceImplementation) and
// xEdit/xeMainForm.pas (GetFormIDCallback) and the core seams
// wbGetCellDetailsForWorldspaceCallback and wbGetFormIDCallback

//! The two prompts of the core seams, answered by dialogs:
//!
//! - `wbGetCellDetailsForWorldspaceCallback` (`xeWorldspaceCellDetailsForm`)
//!   asks whether the world cell being added is persistent or which grid
//!   cell it takes;
//! - `wbGetFormIDCallback` (`xeMainForm.pas`'s "New FormID" `InputQuery`)
//!   asks for the load order FormID of a new record.
//!
//! The seams are synchronous: the core call blocks until it has an answer.
//! The GUI installs callbacks that hand the question to the main form's
//! frame loop and wait for the dialog, which works because the commands
//! that ask (the edit commands) run on the job runner's worker thread. A
//! command that ran on the UI thread would wait for a dialog the same
//! thread must draw, so such a call is refused with the seam's cancelled
//! answer instead of deadlocking.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use xedit_core::interface::File;
use xedit_core::interface::globals;
use xedit_core::interface::{ElementRef, FormID, MainRecordRef};

/// The question of a prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptRequest {
    /// `GetFormIDCallback`: the suggested FormID is the text the input box
    /// opens with.
    FormId { suggested: String },
    /// `TfrmWorldspaceCellDetails` of a worldspace.
    CellDetails { worldspace: String },
}

/// The answer of a prompt; `None`/`Cancelled` is the seam's `False`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptAnswer {
    /// The load order FormID the user typed, `None` for a cancelled box (or
    /// a text that is not a FormID).
    FormId(Option<FormID>),
    /// The persistent flag and the grid cell, or `None` for Cancel.
    CellDetails(Option<(bool, i32, i32)>),
}

impl PromptAnswer {
    fn default_for(request: &PromptRequest) -> Self {
        match request {
            PromptRequest::FormId { .. } => PromptAnswer::FormId(None),
            PromptRequest::CellDetails { .. } => PromptAnswer::CellDetails(None),
        }
    }
}

/// A question waiting for its dialog.
pub struct PendingPrompt {
    pub request: PromptRequest,
    answer: SyncSender<PromptAnswer>,
}

impl PendingPrompt {
    /// Answers the question; the waiting command continues.
    pub fn answer(self, answer: PromptAnswer) {
        let _ = self.answer.send(answer);
    }
}

/// The questions the worker thread asked, in order.
#[derive(Default)]
pub struct PromptQueue {
    pending: Mutex<VecDeque<PendingPrompt>>,
}

impl PromptQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the frame loop and waits for its answer (`wbGetFormID` and
    /// `wbGetCellDetailsForWorldspace` return the answer to the core).
    fn ask(&self, request: PromptRequest) -> PromptAnswer {
        if is_ui_thread() {
            // The UI thread can not wait for a dialog it has to draw.
            return PromptAnswer::default_for(&request);
        }
        let (tx, rx) = sync_channel(1);
        if let Ok(mut pending) = self.pending.lock() {
            pending.push_back(PendingPrompt { request, answer: tx });
        } else {
            return PromptAnswer::default_for(&request);
        }
        rx.recv().unwrap_or_else(|_| {
            PromptAnswer::default_for(&PromptRequest::FormId {
                suggested: String::new(),
            })
        })
    }

    /// A question with no waiting core call (the screen-check route); its
    /// answer is dropped.
    pub fn queue(&self, request: PromptRequest) {
        let (tx, rx) = sync_channel(1);
        drop(rx);
        if let Ok(mut pending) = self.pending.lock() {
            pending.push_back(PendingPrompt { request, answer: tx });
        }
    }

    /// The next question for the frame loop.
    pub fn pop(&self) -> Option<PendingPrompt> {
        self.pending.lock().ok()?.pop_front()
    }

    /// Whether a question waits.
    pub fn is_empty(&self) -> bool {
        self.pending.lock().map(|pending| pending.is_empty()).unwrap_or(true)
    }
}

/// The thread the frame loop runs on (`Application`'s thread upstream).
static UI_THREAD: AtomicU64 = AtomicU64::new(0);

/// A `ThreadId` as an integer, for the atomic.
fn thread_id_of(id: std::thread::ThreadId) -> u64 {
    // `ThreadId` has no stable accessor; its `Debug` text is the same for
    // the same thread within a process.
    format!("{id:?}")
        .trim_start_matches("ThreadId(")
        .trim_end_matches(')')
        .parse::<u64>()
        .unwrap_or(0)
}

/// Names the thread of the frame loop; a prompt asked there is refused
/// instead of deadlocking.
pub fn set_ui_thread() {
    UI_THREAD.store(thread_id_of(std::thread::current().id()), Ordering::Relaxed);
}

fn is_ui_thread() -> bool {
    let ui = UI_THREAD.load(Ordering::Relaxed);
    ui != 0 && thread_id_of(std::thread::current().id()) == ui
}

/// Installs the two core callbacks over `queue` (`initialization
/// wbGetCellDetailsForWorldspaceCallback := ...` of
/// `xeWorldspaceCellDetailsForm.pas` and `FormCreate`'s
/// `wbGetFormIDCallback := GetFormIDCallback`).
pub fn install(queue: &Arc<PromptQueue>) {
    let form_id_queue = Arc::clone(queue);
    globals::set_get_form_id_callback(Some(Arc::new(move |element: &ElementRef| {
        // `ObjectID := aElement._File.NextObjectID`, and the box opens with
        // `FileFormIDtoLoadOrderFormID(NewFormID, True)`.
        let Some(file) = element.as_element_impl().and_then(|element| element.file_impl()) else {
            return FormID::null();
        };
        let remembered = file.get_next_object_id();
        let suggested = file
            .new_form_id()
            .ok()
            .and_then(|new| file.file_form_id_to_load_order_form_id(new, true).ok())
            .map(|form_id| form_id.to_string(false))
            .unwrap_or_default();
        let answer = form_id_queue.ask(PromptRequest::FormId { suggested });
        match answer {
            PromptAnswer::FormId(Some(form_id)) => form_id,
            _ => {
                // `finally if (Result.IsNull) and (ObjectID <> 0) then
                // aElement._File.NextObjectID := ObjectID`: a refused add
                // leaves the file as it was.
                let _ = file.set_next_object_id(remembered);
                FormID::null()
            }
        }
    })));
    let cell_queue = Arc::clone(queue);
    globals::set_get_cell_details_for_worldspace_callback(Some(Arc::new(move |worldspace: &MainRecordRef| {
        let answer = cell_queue.ask(PromptRequest::CellDetails {
            worldspace: worldspace.get_name(),
        });
        match answer {
            PromptAnswer::CellDetails(answer) => answer,
            _ => None,
        }
    })));
}

/// Removes the callbacks (a headless test, or the end of the run).
pub fn uninstall() {
    globals::set_get_form_id_callback(None);
    globals::set_get_cell_details_for_worldspace_callback(None);
}

/// `rbTemporaryClick`'s caption of the worldspace dialog: the form's
/// message is the caption of the form plus the record it adds to.
pub fn cell_details_caption(worldspace: &str) -> String {
    format!("Worldspace Cell: {worldspace}")
}

/// Parses the text of the FormID box (`TwbFormID.FromStr`); an empty box is
/// the null FormID, as upstream's `from_str` of an empty text is.
pub fn parse_form_id(text: &str) -> Option<FormID> {
    let text = text.trim();
    if text.is_empty() {
        Some(FormID::null())
    } else {
        FormID::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prompt_waits_for_its_answer() {
        let _guard = xedit_core::interface::globals::test_lock();
        let queue = Arc::new(PromptQueue::new());
        let asker = Arc::clone(&queue);
        let thread = std::thread::spawn(move || {
            asker.ask(PromptRequest::FormId {
                suggested: "04000000".to_owned(),
            })
        });
        // The frame loop answers it.
        let pending = loop {
            if let Some(pending) = queue.pop() {
                break pending;
            }
            std::thread::yield_now();
        };
        assert_eq!(
            pending.request,
            PromptRequest::FormId {
                suggested: "04000000".to_owned()
            }
        );
        let given = FormID::from_str("0400ABCD").unwrap();
        pending.answer(PromptAnswer::FormId(Some(given)));
        assert_eq!(thread.join().unwrap(), PromptAnswer::FormId(Some(given)));
    }

    #[test]
    fn a_cancelled_prompt_answers_none() {
        let queue = Arc::new(PromptQueue::new());
        let asker = Arc::clone(&queue);
        let thread = std::thread::spawn(move || {
            asker.ask(PromptRequest::CellDetails {
                worldspace: "Tamriel".to_owned(),
            })
        });
        let pending = loop {
            if let Some(pending) = queue.pop() {
                break pending;
            }
            std::thread::yield_now();
        };
        pending.answer(PromptAnswer::CellDetails(None));
        assert_eq!(thread.join().unwrap(), PromptAnswer::CellDetails(None));
    }

    #[test]
    fn the_form_id_text_parses_as_upstream_from_str() {
        assert_eq!(parse_form_id("0400ABCD"), FormID::from_str("0400ABCD"));
        assert_eq!(parse_form_id(""), Some(FormID::null()));
        assert_eq!(parse_form_id("not a form id"), None);
    }

    #[test]
    fn the_worldspace_caption_names_the_record() {
        assert_eq!(cell_details_caption("Tamriel"), "Worldspace Cell: Tamriel");
    }
}
