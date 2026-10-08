// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The threading model of the element tree.
//!
//! Upstream builds the tree on one thread. The port lets several threads
//! read a loaded tree at once (the parallel dump) on these terms:
//!
//! - The definitions and the settings of `globals` are written while a game
//!   is set up and only read afterwards.
//! - A container builds its elements once, on first use ([`InitOnce`]). The
//!   thread that builds them owns the build; another thread that needs the
//!   elements waits until they are complete, so it never sees half a build.
//!   A use from inside the build on the same thread returns at once and sees
//!   the elements built so far, as upstream does with `csInitializing`.
//! - Once built, the elements of a main record do not change until the
//!   record is reset, which the dump does to bound its memory (as upstream
//!   does when the last reference to a record goes). The reset releases the
//!   elements and undoes the init under the lock of the element list, and a
//!   read of the list checks under the same lock that the init is done
//!   (`MainRecordImpl::read_built`), so a thread that reads a record that
//!   another one just reset builds it again. The released elements are
//!   dropped only once no thread that may hold one of them reads any more
//!   ([`retire`], [`ReadGuard`]), so an element of the old build keeps its
//!   links up to its record. A record built only to read its names
//!   gives its elements up before the build counts as done
//!   ([`InitOnce::run_then`]).
//! - The upstream `threadvar`s are per thread here too: the internal edit
//!   count (`globals`) and the resolve guard of an element ([`begin_resolve`]).
//!
//! The work that runs on [`pool`]: the scan of the groups of a plugin
//! (`TwbFile.Scan`; the records are registered with the file and its masters
//! afterwards, on the calling thread, in file order; see
//! `implementation::scan`), and the build and output of the main records of
//! `xedit dump`.
//!
//! Two threads whose builds need each other's elements cannot both wait. The
//! second one to ask sees the other's elements as built so far, which depends
//! on timing; [`init_cycles`] counts these, so that a caller that needs output
//! independent of the thread count can repeat that work on one thread.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

/// The number of threads of the parallel steps, 0 for the default.
static THREADS: AtomicUsize = AtomicUsize::new(0);

/// Sets the number of threads that load and dump: 1 does everything on the
/// calling thread, 0 restores the default (`RAYON_NUM_THREADS`, else one
/// per CPU). The output does not depend on it.
pub fn set_threads(threads: usize) {
    THREADS.store(threads, Ordering::Relaxed);
}

/// The number of threads that load and dump.
pub fn threads() -> usize {
    match THREADS.load(Ordering::Relaxed) {
        0 => std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|threads| threads.trim().parse::<usize>().ok())
            .filter(|threads| *threads > 0)
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |threads| threads.get())),
        threads => threads,
    }
}

/// The stack size of the worker threads: a record resolves deeply through
/// the definitions.
pub const STACK_SIZE: usize = 1 << 30;

/// The worker threads, created on first use with the thread count of that
/// time; `None` when [`threads`] is 1.
pub fn pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    let threads = threads();
    if threads <= 1 {
        return None;
    }
    Some(POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .stack_size(STACK_SIZE)
            .thread_name(|index| format!("xedit-worker-{index}"))
            .build()
            .expect("the worker threads")
    }))
}

/// A token per thread, never 0 or 1.
pub fn thread_token() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(2);
    thread_local! {
        static TOKEN: u64 = NEXT.fetch_add(1, Ordering::Relaxed);
    }
    TOKEN.with(|token| *token)
}

/// The threads that wait for a build, with the thread that owns the build.
static WAITING: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());
static BUILD_DONE: Condvar = Condvar::new();
/// The number of threads in `WAITING`, read without the lock by the thread
/// that finishes a build.
static WAITERS: AtomicUsize = AtomicUsize::new(0);
/// The waits given up because the builds needed each other.
static INIT_CYCLES: AtomicU64 = AtomicU64::new(0);

/// The number of builds whose wait was given up because two threads needed
/// each other's elements. A caller compares it before and after a parallel
/// step to know whether the output of the step may depend on timing.
pub fn init_cycles() -> u64 {
    INIT_CYCLES.load(Ordering::SeqCst)
}

/// Port of the `csInit`, `csInitializing` and `csInitDone` states of
/// `TwbContainer.DoInit`: the initialization runs once, and a call from
/// inside the initialization (a decider that reads the container being
/// built) returns at once instead of blocking. A call from another thread
/// waits until the initialization is done (see the module documentation).
pub struct InitOnce(AtomicU64);

impl InitOnce {
    const NOT_STARTED: u64 = 0;
    const DONE: u64 = 1;
    // Any other value: the token of the thread that runs the init.

    pub const fn new() -> Self {
        InitOnce(AtomicU64::new(Self::NOT_STARTED))
    }

    /// Runs `init` unless it ran or runs already; returns whether this call
    /// ran it.
    pub fn run(&self, init: impl FnOnce()) -> bool {
        self.run_then(|| true, init, || false)
    }

    /// [`run`](Self::run) while `wanted` holds when the init would start,
    /// and then, before another thread can see what `init` built, `release`:
    /// when it returns true, the init counts as not run, and a thread that
    /// waited for it runs it itself if it still wants to.
    pub fn run_then(&self, wanted: impl Fn() -> bool, init: impl FnOnce(), release: impl FnOnce() -> bool) -> bool {
        let me = thread_token();
        loop {
            match self.0.load(Ordering::Acquire) {
                Self::DONE => return false,
                Self::NOT_STARTED => {
                    if !wanted() {
                        return false;
                    }
                    if self
                        .0
                        .compare_exchange(Self::NOT_STARTED, me, Ordering::AcqRel, Ordering::Acquire)
                        .is_err()
                    {
                        continue;
                    }
                    // Waiters are released also when `init` panics.
                    struct Finish<'a>(&'a AtomicU64, u64);
                    impl Drop for Finish<'_> {
                        fn drop(&mut self) {
                            self.0.store(self.1, Ordering::SeqCst);
                            if WAITERS.load(Ordering::SeqCst) > 0 {
                                let _waiting = WAITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                                BUILD_DONE.notify_all();
                            }
                        }
                    }
                    let mut finish = Finish(&self.0, Self::DONE);
                    init();
                    if release() {
                        finish.1 = Self::NOT_STARTED;
                    }
                    return true;
                }
                owner if owner == me => return false,
                owner => {
                    if !self.wait(me, owner) {
                        return false;
                    }
                }
            }
        }
    }

    /// Waits until the init that `owner` runs is done. Returns false without
    /// waiting when `owner` waits, directly or through other threads, for an
    /// init of this thread.
    fn wait(&self, me: u64, owner: u64) -> bool {
        let mut waiting = WAITING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        WAITERS.fetch_add(1, Ordering::SeqCst);
        let mut result = true;
        loop {
            let current = self.0.load(Ordering::SeqCst);
            if current != owner {
                break;
            }
            // Follow the threads `owner` waits for.
            let mut next = owner;
            let mut cycle = false;
            for _ in 0..=waiting.len() {
                if next == me {
                    cycle = true;
                    break;
                }
                match waiting.iter().find(|(waiter, _)| *waiter == next) {
                    Some((_, owner)) => next = *owner,
                    None => break,
                }
            }
            if cycle {
                INIT_CYCLES.fetch_add(1, Ordering::SeqCst);
                result = false;
                break;
            }
            waiting.push((me, owner));
            waiting = BUILD_DONE
                .wait(waiting)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(index) = waiting.iter().position(|(waiter, _)| *waiter == me) {
                waiting.swap_remove(index);
            }
        }
        WAITERS.fetch_sub(1, Ordering::SeqCst);
        result
    }

    /// Whether an init runs (`csInit`), on any thread.
    pub fn is_running(&self) -> bool {
        !matches!(self.0.load(Ordering::Acquire), Self::NOT_STARTED | Self::DONE)
    }

    /// Whether the init is done.
    pub fn is_done(&self) -> bool {
        self.0.load(Ordering::Acquire) == Self::DONE
    }

    /// Whether this thread runs the init: a call from inside it.
    pub fn is_running_here(&self) -> bool {
        self.0.load(Ordering::Acquire) == thread_token()
    }

    /// Port of the `csInitDone` removal in `DoReset`: the next `run` builds
    /// again. Nothing happens while an init runs or before one ran; returns
    /// whether a finished init was reset.
    pub fn reset(&self) -> bool {
        self.0
            .compare_exchange(Self::DONE, Self::NOT_STARTED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

impl Default for InitOnce {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    /// The elements this thread resolves a definition through (`esResolving`).
    static RESOLVING: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Port of `BeginResolve` for the element at `address`: false while this
/// thread resolves through the element already. Another thread may resolve
/// through the same element at the same time; the guard only stops the
/// recursion of one thread.
pub fn begin_resolve(address: usize) -> bool {
    RESOLVING.with(|resolving| {
        let mut resolving = resolving.borrow_mut();
        if resolving.contains(&address) {
            false
        } else {
            resolving.push(address);
            true
        }
    })
}

/// Port of `EndResolve`.
pub fn end_resolve(address: usize) {
    RESOLVING.with(|resolving| {
        let mut resolving = resolving.borrow_mut();
        if let Some(index) = resolving.iter().rposition(|known| *known == address) {
            resolving.remove(index);
        }
    });
}

/// The number of reader slots: one per thread, in the order the threads
/// first read (two threads share a slot only beyond this many).
const READER_SLOTS: usize = 256;
const INACTIVE: u64 = u64::MAX;

/// A reader slot on a cache line of its own.
#[repr(align(64))]
struct ReaderSlot(AtomicU64);

/// The epoch each thread entered its [`ReadGuard`] at, or `INACTIVE`.
static READERS: [ReaderSlot; READER_SLOTS] = [const { ReaderSlot(AtomicU64::new(INACTIVE)) }; READER_SLOTS];
/// The epoch, advanced by every [`retire`].
static EPOCH: AtomicU64 = AtomicU64::new(0);
/// The elements a reset released while a reader may still hold them, with
/// the epoch of the release.
type Retired = (u64, Box<dyn Send>);
static RETIRED: Mutex<std::collections::VecDeque<Retired>> = Mutex::new(std::collections::VecDeque::new());

fn reader_slot() -> &'static AtomicU64 {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    thread_local! {
        static SLOT: usize = NEXT.fetch_add(1, Ordering::Relaxed) % READER_SLOTS;
    }
    &READERS[SLOT.with(|slot| *slot)].0
}

/// While a guard lives, the elements of a record that another thread resets
/// stay alive, with their links to their containers ([`retire`]). The dump
/// holds one while it computes a line, which may read other records.
pub struct ReadGuard(Option<&'static AtomicU64>);

/// Enters a [`ReadGuard`]; a guard inside another one of the same thread
/// changes nothing.
pub fn read_guard() -> ReadGuard {
    let slot = reader_slot();
    if slot.load(Ordering::SeqCst) != INACTIVE {
        return ReadGuard(None);
    }
    slot.store(EPOCH.load(Ordering::SeqCst), Ordering::SeqCst);
    ReadGuard(Some(slot))
}

impl Drop for ReadGuard {
    fn drop(&mut self) {
        if let Some(slot) = self.0 {
            slot.store(INACTIVE, Ordering::SeqCst);
        }
    }
}

/// Port of the release of the elements of a reset container: `garbage`
/// (the released elements) is dropped once no [`ReadGuard`] that was entered
/// before the release is alive, so that a thread that reads an element of
/// the reset record keeps its links up to the record.
pub fn retire(garbage: impl Send + 'static) {
    let epoch = EPOCH.fetch_add(1, Ordering::SeqCst);
    if oldest_reader() > epoch {
        drop(garbage);
    } else {
        RETIRED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back((epoch, Box::new(garbage)));
    }
    collect_retired();
}

/// The oldest epoch a [`ReadGuard`] alive was entered at.
fn oldest_reader() -> u64 {
    READERS
        .iter()
        .map(|slot| slot.0.load(Ordering::SeqCst))
        .min()
        .unwrap_or(INACTIVE)
}

/// Drops the retired elements that no reader can hold any more.
pub fn collect_retired() {
    let oldest = oldest_reader();
    let expired: Vec<Retired> = {
        let Ok(mut retired) = RETIRED.try_lock() else { return };
        let count = retired.iter().take_while(|(epoch, _)| *epoch < oldest).count();
        retired.drain(..count).collect()
    };
    drop(expired);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn init_runs_once_and_reports_who_ran_it() {
        let once = InitOnce::new();
        let mut count = 0;
        assert!(once.run(|| count += 1));
        assert!(!once.run(|| count += 1));
        assert_eq!(count, 1);
        assert!(once.reset());
        assert!(once.run(|| count += 1));
        assert_eq!(count, 2);
    }

    #[test]
    fn a_call_from_inside_the_init_returns_at_once() {
        let once = InitOnce::new();
        let mut inner_ran = false;
        once.run(|| {
            assert!(once.is_running_here());
            inner_ran = once.run(|| unreachable!());
        });
        assert!(!inner_ran);
        assert!(!once.is_running());
    }

    #[test]
    fn another_thread_waits_for_the_init() {
        let once = Arc::new(InitOnce::new());
        let built = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let owner = {
            let once = once.clone();
            let built = built.clone();
            std::thread::spawn(move || {
                once.run(|| {
                    started_tx.send(()).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    built.store(true, Ordering::SeqCst);
                });
            })
        };
        started_rx.recv().unwrap();
        assert!(once.is_running() && !once.is_running_here());
        assert!(!once.run(|| unreachable!()));
        assert!(built.load(Ordering::SeqCst), "the waiter saw the finished build");
        owner.join().unwrap();
    }

    #[test]
    fn a_released_init_is_run_again_by_the_thread_that_waited() {
        let once = Arc::new(InitOnce::new());
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let owner = {
            let once = once.clone();
            std::thread::spawn(move || {
                once.run_then(
                    || true,
                    || {
                        started_tx.send(()).unwrap();
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    },
                    || true,
                )
            })
        };
        started_rx.recv().unwrap();
        let mut ran = false;
        assert!(once.run(|| ran = true));
        assert!(ran, "the waiter built after the release");
        assert!(owner.join().unwrap());
    }

    #[test]
    fn two_builds_that_need_each_other_do_not_deadlock() {
        let a = Arc::new(InitOnce::new());
        let b = Arc::new(InitOnce::new());
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let before = init_cycles();
        let spawn = |first: Arc<InitOnce>, second: Arc<InitOnce>, barrier: Arc<std::sync::Barrier>| {
            std::thread::spawn(move || {
                first.run(|| {
                    barrier.wait();
                    second.run(|| {});
                });
            })
        };
        let one = spawn(a.clone(), b.clone(), barrier.clone());
        let two = spawn(b.clone(), a.clone(), barrier);
        one.join().unwrap();
        two.join().unwrap();
        assert_eq!(init_cycles() - before, 1);
    }

    #[test]
    fn retired_elements_live_while_an_older_reader_reads() {
        struct Flag(Arc<AtomicBool>);
        impl Drop for Flag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = read_guard();
        let retiring = Flag(dropped.clone());
        std::thread::spawn(move || retire(retiring)).join().unwrap();
        assert!(!dropped.load(Ordering::SeqCst), "kept while the reader reads");
        drop(guard);
        collect_retired();
        assert!(dropped.load(Ordering::SeqCst), "dropped once the reader is done");
    }

    #[test]
    fn the_resolve_guard_is_per_thread() {
        assert!(begin_resolve(1));
        assert!(!begin_resolve(1));
        std::thread::spawn(|| {
            assert!(begin_resolve(1));
            end_resolve(1);
        })
        .join()
        .unwrap();
        end_resolve(1);
        assert!(begin_resolve(1));
        end_resolve(1);
    }
}
