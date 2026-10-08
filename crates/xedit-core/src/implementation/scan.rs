// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! The scan of a plugin on the worker threads of `crate::threads`.
//!
//! `TwbFile.Scan` reads the records and groups one after the other. Each
//! one is added to its container as it is read, and each main record
//! registers itself with the file (`AddMainRecord`: the record list, the
//! override of a master's record, an injected record). Reading a group does
//! not depend on the groups before it, so the large groups are read on the
//! workers, down to the groups inside them. What the serial scan does on the
//! way that reaches outside of a task (adding an element to a container that
//! other tasks add to as well, the registration, the progress messages) is
//! recorded in order instead, and merged into the task that started it; the
//! calling thread replays the registrations and messages of the whole file
//! in file order. A task adds the elements of the groups it created itself
//! at once. The file and its masters therefore end up as the serial scan
//! leaves them.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use rayon::prelude::*;

use super::structs::{GroupRecordStruct, MainRecordStruct};
use super::{ElementImplCasts, FileImpl, LoadError, MainRecordImpl, create_record};
use crate::interface::element::ElementRef;
use crate::interface::globals::size_of_main_record_struct;
use crate::interface::misc::progress as report;

/// What the scan did outside of the element being created, in order.
enum Event {
    Attach(ElementRef, ElementRef),
    Progress(String),
    Register(Arc<MainRecordImpl>),
}

/// The part of the scan one task runs, while the scan runs on the workers.
#[derive(Default)]
struct Frame {
    /// The events, for the task that started this one.
    events: Vec<Event>,
    /// The addresses of the groups this task created. No other task adds
    /// to them, so their children are added at once.
    own: HashSet<usize>,
}

thread_local! {
    static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
}

fn address(element: &ElementRef) -> usize {
    Arc::as_ptr(element).cast::<()>() as usize
}

/// Records `event` when the scan runs on the workers; returns it otherwise.
fn record(event: Event) -> Option<Event> {
    FRAME.with_borrow_mut(|frame| match frame {
        Some(frame) => {
            frame.events.push(event);
            None
        }
        None => Some(event),
    })
}

/// Adds `element` to `container`: at once when no other task adds to the
/// container, else recorded.
pub(super) fn attach(container: &ElementRef, element: ElementRef) {
    let is_group = element
        .as_element_impl()
        .is_some_and(|element| element.group_record_impl().is_some());
    let event = FRAME.with_borrow_mut(|frame| match frame {
        Some(frame) => {
            if is_group {
                frame.own.insert(address(&element));
            }
            if frame.own.contains(&address(container)) {
                Some(Event::Attach(container.clone(), element))
            } else {
                frame.events.push(Event::Attach(container.clone(), element));
                None
            }
        }
        None => Some(Event::Attach(container.clone(), element)),
    });
    if let Some(event) = event {
        replay(event);
    }
}

/// `wbProgress` during the scan.
pub(super) fn progress(message: &str) {
    if let Some(event) = record(Event::Progress(message.to_owned())) {
        replay(event);
    }
}

/// Whether the registration of `record` with its file (`AddMainRecord`) is
/// recorded for later, because the scan runs on the workers.
pub(super) fn defer_registration(main_record: &Arc<MainRecordImpl>) -> bool {
    record(Event::Register(main_record.clone())).is_none()
}

fn replay(event: Event) {
    match event {
        Event::Attach(container, element) => {
            if let Some(parent) = container.as_container_base() {
                parent.add_element(element);
            }
        }
        Event::Progress(message) => report(&message),
        Event::Register(record) => {
            if let Some(file) = record.file_impl() {
                file.register_scanned(&record);
            }
        }
    }
}

/// Runs `scan` in a frame of its own and returns its events. The frame of
/// the caller is kept aside meanwhile: a worker that waits for its nested
/// tasks runs other tasks on its thread.
fn recorded<T>(scan: impl FnOnce() -> T) -> (T, Vec<Event>) {
    let outer = FRAME.replace(Some(Frame::default()));
    let result = scan();
    let events = FRAME.replace(outer).map(|frame| frame.events).unwrap_or_default();
    (result, events)
}

/// Takes the events of a nested task into the frame of this thread, in
/// order: the children of the groups this task created are added now.
fn merge(events: Vec<Event>) {
    let ready: Vec<Event> = FRAME.with_borrow_mut(|frame| {
        let frame = frame.get_or_insert_with(Frame::default);
        let mut ready = Vec::new();
        for event in events {
            match event {
                Event::Attach(container, element) if frame.own.contains(&address(&container)) => {
                    ready.push(Event::Attach(container, element));
                }
                event => frame.events.push(event),
            }
        }
        ready
    });
    for event in ready {
        replay(event);
    }
}

/// A child group from this size on is read on a worker of its own.
const LARGE_GROUP: usize = 128 << 10;

/// Port of the loop of `TwbFile.Scan` over the records after the header:
/// the top level groups from `offset` to the end of the file, on the
/// workers when there is more than one thread.
pub(super) fn scan_top_level(
    file: &Arc<FileImpl>,
    container: &ElementRef,
    bytes: &[u8],
    offset: &mut usize,
) -> Result<(), LoadError> {
    let Some(pool) = crate::threads::pool() else {
        *offset = scan_records(file, container, bytes, *offset, bytes.len())?;
        return Ok(());
    };
    let start = *offset;
    let (result, events) = pool.install(|| recorded(|| scan_records(file, container, bytes, start, bytes.len())));
    // The registration of a record looks at the records registered before
    // it, so the events are replayed in file order on this thread.
    for event in events {
        replay(event);
    }
    *offset = result?;
    Ok(())
}

/// Port of `TwbGroupRecord.ScanData` and of the loop of `TwbFile.Scan`: the
/// records and groups from `start` until `end` is reached, in `container`.
/// Returns the offset after the last one. While the events are recorded
/// (`scan_top_level`), the large child groups are read on the workers.
pub(super) fn scan_records(
    file: &Arc<FileImpl>,
    container: &ElementRef,
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<usize, LoadError> {
    let recording = FRAME.with_borrow(Option::is_some);
    let pieces = if recording { split(bytes, start, end) } else { None };
    let Some(pieces) = pieces.filter(|pieces| pieces.len() > 1) else {
        return scan_serially(file, container, bytes, start, end);
    };
    let scanned: Vec<(Result<usize, LoadError>, Vec<Event>)> = pieces
        .par_iter()
        .with_max_len(1)
        .map(|piece| recorded(|| scan_serially(file, container, bytes, piece.start, piece.end)))
        .collect();
    let mut offset = start;
    for (result, events) in scanned {
        merge(events);
        offset = result?;
    }
    Ok(offset)
}

/// The serial loop: each record after the previous one.
fn scan_serially(
    file: &Arc<FileImpl>,
    container: &ElementRef,
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<usize, LoadError> {
    let mut current = start;
    let mut prev_main_record: Option<Arc<MainRecordImpl>> = None;
    while current < end {
        let record = create_record(file, container, bytes, &mut current, prev_main_record.as_ref())?;
        prev_main_record = record.and_then(|record| record.into_main_record_impl());
    }
    Ok(current)
}

/// A part of the records of a container that one task reads.
struct Piece {
    start: usize,
    end: usize,
}

/// The records from `start` to `end` cut into pieces: each large group on
/// its own, and the records between them together. A main record is checked
/// for a duplicate FormID against the record before it, which a group in
/// between resets, so no piece needs the record before it. `None` when a
/// header cannot be read: the serial loop then reports the error.
fn split(bytes: &[u8], start: usize, end: usize) -> Option<Vec<Piece>> {
    let header_size = size_of_main_record_struct() as usize;
    let mut pieces = Vec::new();
    let mut run_start = start;
    let mut offset = start;
    while offset < end {
        let next;
        if bytes.get(offset..offset + 4)? == b"GRUP" {
            let size = GroupRecordStruct::parse(bytes, offset)?.group_size as usize;
            if size < header_size {
                return None;
            }
            next = (offset + size).min(bytes.len());
            if size >= LARGE_GROUP {
                if run_start < offset {
                    pieces.push(Piece {
                        start: run_start,
                        end: offset,
                    });
                }
                pieces.push(Piece {
                    start: offset,
                    end: next,
                });
                run_start = next;
            }
        } else {
            let data_size = MainRecordStruct::parse(bytes, offset)?.data_size as usize;
            next = (offset + header_size + data_size).min(bytes.len());
        }
        offset = next;
    }
    if run_start < offset {
        pieces.push(Piece {
            start: run_start,
            end: offset,
        });
    }
    Some(pieces)
}
