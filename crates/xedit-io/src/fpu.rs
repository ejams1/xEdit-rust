// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The floating point control word a script can change (`Math.SetRoundMode`
//! reaches the `MXCSR` rounding bits of the thread on x86-64).
//!
//! Rust's text-to-double conversion takes a fast path that divides or
//! multiplies in `f64` (`0.1` is `1.0 / 10.0`), so under a directed mode it
//! returns the neighbour of the correctly rounded value. The oracle's
//! conversions do not follow the mode (`0.1 + 0.2` under `rmDown` adds the
//! nearest `0.1` and `0.2`, the `sys-round` probe), so every conversion of
//! text runs through [`with_nearest_rounding`].

/// The two rounding-control bits of the `MXCSR` control word.
#[cfg(target_arch = "x86_64")]
const MXCSR_ROUNDING: u32 = 0x6000;

/// Runs `body` with the rounding bits at round-to-nearest and puts the
/// thread's word back. Free when the mode is nearest already.
pub fn with_nearest_rounding<R>(body: impl FnOnce() -> R) -> R {
    #[cfg(target_arch = "x86_64")]
    {
        let mut saved: u32 = 0;
        // SAFETY: `stmxcsr` only reads the control word of the calling
        // thread into `saved`.
        unsafe {
            core::arch::asm!("stmxcsr [{0}]", in(reg) &mut saved, options(nostack, preserves_flags));
        }
        if saved & MXCSR_ROUNDING == 0 {
            return body();
        }
        let nearest = saved & !MXCSR_ROUNDING;
        // SAFETY: `ldmxcsr` writes the control word of the calling thread
        // with a value derived from its own; the original is restored
        // below before the thread does anything else.
        unsafe {
            core::arch::asm!("ldmxcsr [{0}]", in(reg) &nearest, options(nostack, preserves_flags));
        }
        let result = body();
        // SAFETY: as above; `saved` is the word read before.
        unsafe {
            core::arch::asm!("ldmxcsr [{0}]", in(reg) &saved, options(nostack, preserves_flags));
        }
        result
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        body()
    }
}

/// Parses a double the way the oracle's conversions do, outside the
/// thread's rounding mode.
pub fn parse_f64(text: &str) -> Option<f64> {
    with_nearest_rounding(|| text.parse::<f64>().ok())
}
