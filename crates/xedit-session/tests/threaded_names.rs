// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The names a record caches read while another thread builds it (issue
//! #99), forced through the progress callback that `Init` reports an
//! unknown subrecord with.
//!
//! - A name read while the record's init runs on another thread and the two
//!   builds need each other (`threads::init_cycles`) answers the "not yet
//!   available" text and is not cached, so the record's own name is built
//!   from its subrecords afterwards, as single-threaded upstream builds it.
//!   Both records have the unknown subrecord first, and each thread's
//!   callback parks until the other one is inside its own record's init,
//!   then reads the other record's name: one of the two waits is given up.
//! - A language change (`mrInvalidateNameCache`) while another thread's
//!   init runs keeps the full name the init already read: the record has
//!   the unknown subrecord after its `FULL`, so the callback parks in the
//!   window between the name and the end of the init, and the first name
//!   read of the record invalidates the cache there.

use std::cell::Cell;
use std::sync::{Arc, Barrier, Mutex};

use xedit_core::implementation::{FileBytes, clear_files_map, reset_load_order_slots, wb_file_from_bytes};
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::misc::set_progress_callback;
use xedit_core::interface::types::FileStates;
use xedit_core::interface::{File, FormID};

fn sub_record(signature: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u16).to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn main_record(signature: &[u8; 4], form_id: u32, data: &[u8]) -> Vec<u8> {
    main_record_flags(signature, form_id, 0, data)
}

/// A main record with the flags of its header, which mark a `TES4` file
/// localized (`0x80`).
fn main_record_flags(signature: &[u8; 4], form_id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = signature.to_vec();
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&flags.to_le_bytes());
    bytes.extend_from_slice(&form_id.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&44u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn top_group(label: &[u8; 4], records: &[u8]) -> Vec<u8> {
    let mut bytes = b"GRUP".to_vec();
    bytes.extend_from_slice(&((24 + records.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(label);
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(records);
    bytes
}

/// The subrecords of a record: one the definition does not know first, so
/// `Init` reports an error before it reads `EDID` and `FULL`, then the two
/// names.
fn record_data(editor_id: &str, full_name: &str) -> Vec<u8> {
    let mut data = sub_record(b"ZZZZ", &[1, 2, 3, 4]);
    data.extend(sub_record(b"EDID", format!("{editor_id}\0").as_bytes()));
    data.extend(sub_record(b"FULL", format!("{full_name}\0").as_bytes()));
    data
}

/// A game master (an official file, whose records cache their names) with
/// one class and one misc item.
fn master_bytes() -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&1u32.to_le_bytes());
    hedr.extend_from_slice(&0x803u32.to_le_bytes());
    let mut bytes = main_record(b"TES4", 1, &sub_record(b"HEDR", &hedr));
    bytes.extend(top_group(
        b"CLAS",
        &main_record(b"CLAS", 0x800, &record_data("TestClass", "The Test Class")),
    ));
    bytes.extend(top_group(
        b"MISC",
        &main_record(b"MISC", 0x801, &record_data("TestItem", "The Test Item")),
    ));
    bytes
}

/// A game master marked localized (the `0x80` of its header flags), with two
/// classes whose `FULL` is the lstring `0x12345678` and an unknown subrecord
/// after it: `Init` reports the unknown subrecord once it read the name.
fn localized_master_bytes() -> Vec<u8> {
    let mut hedr = Vec::new();
    hedr.extend_from_slice(&1.7f32.to_le_bytes());
    hedr.extend_from_slice(&1u32.to_le_bytes());
    hedr.extend_from_slice(&0x881u32.to_le_bytes());
    let class = |editor_id: &str| {
        let mut data = sub_record(b"EDID", format!("{editor_id}\0").as_bytes());
        data.extend(sub_record(b"FULL", &0x1234_5678u32.to_le_bytes()));
        data.extend(sub_record(b"ZZZZ", &[1, 2, 3, 4]));
        data
    };
    // The localized flag of the file's header (`fsIsLocalized`).
    let mut bytes = main_record_flags(b"TES4", 1, 0x81, &sub_record(b"HEDR", &hedr));
    bytes.extend(top_group(b"CLAS", &main_record(b"CLAS", 0x800, &class("TestClass"))));
    bytes.extend(top_group(b"CLAS", &main_record(b"CLAS", 0x801, &class("ControlClass"))));
    bytes
}

#[test]
fn invalidating_the_name_cache_keeps_the_full_name_of_a_running_init() {
    let _guard = test_lock();
    clear_files_map();
    reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    let file = wb_file_from_bytes(
        "Skyrim.esm",
        0,
        FileStates::empty(),
        FileBytes::Owned(localized_master_bytes()),
    )
    .unwrap();
    let record = |form_id: u32| {
        file.get_record_by_form_id(FormID::from_cardinal(form_id), false, false)
            .unwrap()
            .expect("the class")
    };
    let raced = record(0x800);
    let control = record(0x801);

    thread_local! {
        /// Whether the progress callback still has to park this thread.
        static ARMED: Cell<bool> = const { Cell::new(false) };
    }

    let (parked_tx, parked_rx) = std::sync::mpsc::channel();
    let (go_tx, go_rx) = std::sync::mpsc::channel();
    let go = Arc::new(Mutex::new(go_rx));
    set_progress_callback(Some(Arc::new(move |message: &str| {
        if !message.starts_with("Error: record ") || !ARMED.with(|armed| armed.replace(false)) {
            return;
        }
        // The record's `FULL` is read already, and `mr_names_known` is not
        // set yet: the window the invalidation used to clear the name in.
        parked_tx.send(()).unwrap();
        go.lock().unwrap().recv().unwrap();
        // The other thread is in the invalidation now.
        std::thread::sleep(std::time::Duration::from_millis(100));
    })));

    let raced_thread = raced.clone();
    let init = std::thread::spawn(move || {
        ARMED.with(|armed| armed.set(true));
        raced_thread.get_name()
    });
    parked_rx.recv().unwrap();
    // The handler arrives while the init runs, as the dump's resources do
    // after the load: every name read before it has to be read again.
    xedit_core::localization::install_localization_handler();
    go_tx.send(()).unwrap();
    // The first name read of the record invalidates its cache while the
    // other thread's init is past its `FULL`.
    let name = raced.get_name();
    let name_from_init = init.join().unwrap();
    set_progress_callback(None);

    // The control record reads the same lstring: its name carries the text
    // the init of the raced record read.
    let control_name = control.get_name();
    let quoted = control_name
        .split_once('"')
        .and_then(|(_, rest)| rest.rsplit_once('"'))
        .map(|(text, _)| text.to_owned())
        .expect("the control record has a full name");
    assert!(!quoted.is_empty());
    let expected = format!("CLAS - Class [00000800] <TestClass> \"{quoted}\"");
    assert_eq!(name_from_init, expected, "the init keeps the full name it read");
    assert_eq!(name, expected, "the name read meanwhile has the full name");
    assert_eq!(raced.get_name(), expected, "the invalidated cache reads the name again");
    clear_files_map();
}

#[test]
fn a_name_read_during_another_threads_quick_init_is_not_cached() {
    let _guard = test_lock();
    clear_files_map();
    reset_load_order_slots();
    xedit_core::interface::clear_files();
    xedit_session::dump::setup_game("sse").unwrap();
    let file = wb_file_from_bytes("Skyrim.esm", 0, FileStates::empty(), FileBytes::Owned(master_bytes())).unwrap();
    let class = file
        .get_record_by_form_id(FormID::from_cardinal(0x800), false, false)
        .unwrap()
        .expect("the class");
    let misc = file
        .get_record_by_form_id(FormID::from_cardinal(0x801), false, false)
        .unwrap()
        .expect("the misc item");

    thread_local! {
        /// Which record's init this thread runs: 1 for the class, 2 for the misc item.
        static ROLE: Cell<u8> = const { Cell::new(0) };
        /// Whether the callback already used this thread.
        static USED: Cell<bool> = const { Cell::new(false) };
    }

    let both_inside = Arc::new(Barrier::new(2));
    let names: Arc<Mutex<(Option<String>, Option<String>)>> = Arc::new(Mutex::new((None, None)));
    let class_read = class.clone();
    let misc_read = misc.clone();
    let barrier = both_inside.clone();
    let results = names.clone();
    set_progress_callback(Some(Arc::new(move |message: &str| {
        // The unexpected subrecord of each record's own init.
        if !message.starts_with("Error: record ") {
            return;
        }
        let role = ROLE.with(Cell::get);
        if role == 0 || USED.with(|used| used.replace(true)) {
            return;
        }
        // The other thread is inside its own record's init now; both wait
        // until this one is too, then read the other record's name. The
        // two quick inits need each other, so one wait is given up.
        barrier.wait();
        let name = match role {
            1 => misc_read.get_name(),
            _ => class_read.get_name(),
        };
        let mut names = results.lock().unwrap();
        match role {
            1 => names.0 = Some(name),
            _ => names.1 = Some(name),
        }
    })));

    let class_thread = class.clone();
    let one = std::thread::spawn(move || {
        ROLE.with(|role| role.set(1));
        class_thread.get_name()
    });
    let misc_thread = misc.clone();
    let two = std::thread::spawn(move || {
        ROLE.with(|role| role.set(2));
        misc_thread.get_name()
    });
    let one_name = one.join().unwrap();
    let two_name = two.join().unwrap();
    set_progress_callback(None);

    // Each callback read the other record's name; one of the two waits was
    // given up (the builds need each other), and that read answered the
    // "not yet available" text instead of a partial name.
    let names = names.lock().unwrap();
    let read = [names.0.as_deref(), names.1.as_deref()];
    assert!(read.iter().all(Option::is_some), "both callbacks read a name: {read:?}");
    assert_eq!(
        read.iter()
            .flatten()
            .filter(|name| name.contains("not yet available"))
            .count(),
        1,
        "one wait was given up and answered the not-yet-available text: {read:?}"
    );

    // Neither record cached the partial name: their own builds finish with
    // the `EDID` and `FULL` of the record, and so does every name read
    // afterwards, whichever thread read the name while the init ran.
    assert_eq!(one_name, "CLAS - Class [00000800] <TestClass> \"The Test Class\"");
    assert_eq!(two_name, "MISC - Misc. Item [00000801] <TestItem> \"The Test Item\"");
    assert_eq!(class.get_name(), one_name);
    assert_eq!(misc.get_name(), two_name);
    clear_files_map();
}
