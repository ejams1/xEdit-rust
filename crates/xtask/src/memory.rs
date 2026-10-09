// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Memory limits for the processes the parity harness starts.
//!
//! Each oracle and port process runs in a Windows job object that caps its
//! committed memory, so a runaway dump fails on its own instead of pushing
//! the machine out of memory, and that reports the peak the process used.
//! [`Budget`] keeps the sum of the expected peaks of the running processes
//! under a limit, so that large files do not run side by side.

use std::sync::{Condvar, Mutex};

pub const GIB: u64 = 1 << 30;

/// A memory cap on one child process.
pub struct Limit {
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
    limit: u64,
}

// SAFETY: a job object handle may be used from any thread.
#[cfg(windows)]
unsafe impl Send for Limit {}

impl Limit {
    /// Puts `child` in a new job object that caps its committed memory at
    /// `limit` bytes. Killing the harness closes the job and kills the child.
    #[cfg(windows)]
    pub fn apply(child: &impl std::os::windows::io::AsRawHandle, limit: u64) -> anyhow::Result<Self> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        // SAFETY: plain Win32 calls on handles owned here or by `child`; the
        // information structure lives for the duration of the call.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            anyhow::ensure!(
                !job.is_null(),
                "CreateJobObjectW failed: {}",
                std::io::Error::last_os_error()
            );
            let limit_guard = Self { job, limit };
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            info.ProcessMemoryLimit = usize::try_from(limit).unwrap_or(usize::MAX);
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            anyhow::ensure!(
                set != 0,
                "SetInformationJobObject failed: {}",
                std::io::Error::last_os_error()
            );
            let assigned = AssignProcessToJobObject(job, child.as_raw_handle());
            anyhow::ensure!(
                assigned != 0,
                "AssignProcessToJobObject failed: {}",
                std::io::Error::last_os_error()
            );
            Ok(limit_guard)
        }
    }

    #[cfg(not(windows))]
    pub fn apply<T>(_child: &T, limit: u64) -> anyhow::Result<Self> {
        Ok(Self { limit })
    }

    /// The largest committed memory of the process so far, `None` where
    /// the platform does not report it.
    #[cfg(windows)]
    pub fn peak(&self) -> Option<u64> {
        use windows_sys::Win32::System::JobObjects::{
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, QueryInformationJobObject,
        };
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // SAFETY: the job handle is open and the structure is large enough.
        let queried = unsafe {
            QueryInformationJobObject(
                self.job,
                JobObjectExtendedLimitInformation,
                (&raw mut info).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        (queried != 0).then_some(info.PeakProcessMemoryUsed as u64)
    }

    #[cfg(not(windows))]
    pub fn peak(&self) -> Option<u64> {
        None
    }

    /// The user and kernel CPU time of every process of the job (the child
    /// and the processes it started) in 100 ns units.
    #[cfg(windows)]
    pub fn cpu_time(&self) -> u64 {
        use windows_sys::Win32::System::JobObjects::{
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation, QueryInformationJobObject,
        };
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: the job handle is open and the structure is large enough.
        let queried = unsafe {
            QueryInformationJobObject(
                self.job,
                JobObjectBasicAccountingInformation,
                (&raw mut info).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if queried == 0 {
            return 0;
        }
        (info.TotalUserTime + info.TotalKernelTime) as u64
    }

    #[cfg(not(windows))]
    pub fn cpu_time(&self) -> u64 {
        0
    }

    /// Whether the process came within 1% of the cap, which means a
    /// failure was caused by the cap and says nothing about the dump.
    pub fn reached(&self) -> bool {
        self.peak().is_some_and(|peak| peak >= self.limit - self.limit / 100)
    }
}

#[cfg(windows)]
impl Drop for Limit {
    fn drop(&mut self) {
        // SAFETY: the handle was created by `apply` and is closed once.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.job) };
    }
}

/// Installed physical memory in bytes, `None` where it is not known.
#[cfg(windows)]
pub fn physical_memory() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `dwLength` is set as the call requires.
    (unsafe { GlobalMemoryStatusEx(&raw mut status) } != 0).then_some(status.ullTotalPhys)
}

#[cfg(not(windows))]
pub fn physical_memory() -> Option<u64> {
    None
}

/// Shared memory budget of the running processes. A reservation waits until
/// it fits, except when nothing else runs: a file whose estimate exceeds the
/// whole budget then runs alone.
pub struct Budget {
    total: u64,
    used: Mutex<u64>,
    freed: Condvar,
}

pub struct Reservation<'a> {
    budget: &'a Budget,
    bytes: u64,
}

impl Budget {
    pub fn new(total: u64) -> Self {
        Self {
            total,
            used: Mutex::new(0),
            freed: Condvar::new(),
        }
    }

    pub fn reserve(&self, bytes: u64) -> Reservation<'_> {
        let mut used = self.used.lock().unwrap();
        while *used > 0 && *used + bytes > self.total {
            used = self.freed.wait(used).unwrap();
        }
        *used += bytes;
        Reservation { budget: self, bytes }
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        *self.budget.used.lock().unwrap() -= self.bytes;
        self.budget.freed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_reservation_runs_alone() {
        let budget = Budget::new(10);
        let big = budget.reserve(25);
        drop(big);
        let a = budget.reserve(6);
        let b = std::thread::scope(|scope| {
            let waiter = scope.spawn(|| {
                let _b = budget.reserve(6);
                *budget.used.lock().unwrap()
            });
            std::thread::sleep(std::time::Duration::from_millis(50));
            drop(a);
            waiter.join().unwrap()
        });
        assert_eq!(b, 6);
    }

    #[cfg(windows)]
    #[test]
    fn limit_reports_peak() {
        let child = std::process::Command::new("cmd").args(["/c", "exit"]).spawn().unwrap();
        let limit = Limit::apply(&child, 4 * GIB);
        let mut child = child;
        child.wait().unwrap();
        // The child may exit before it is assigned to the job.
        if let Ok(limit) = limit {
            assert!(!limit.reached());
        }
    }
}
