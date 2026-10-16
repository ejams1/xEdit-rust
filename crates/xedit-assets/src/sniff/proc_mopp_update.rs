// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcMoppUpdate.pas

//! `Update MOPP code`: the MOPP collision code of every
//! `bhkMoppBvTreeShape` is generated again by `NifMopp.dll`, Havok's MOPP
//! builder, and written into `MOPP Data`, `Scale` and `Origin`.
//!
//! Dependency and distribution strategy: `NifMopp.dll` is Havok's library.
//! It is not part of the xEdit source, is not in the release archive and is
//! not redistributable, and the released 4.1.5q `Sniff.exe` is a 64-bit
//! program, whose `LoadMoppDll` raises
//! `You are running 64 bits version of Sniff. This operation works in 32
//! bits (x86) Sniff only.` before it looks for the DLL. The port follows
//! the release: on a 64-bit build `on_start` gives that message and the run
//! stops before any file, and no 32-bit build or DLL is shipped, so a
//! build that reached the loader would give `Unable to load NifMopp.dll`
//! as upstream does when the DLL is missing. The body of `ProcessFile`,
//! which needs the DLL, is not ported: no released binary can run it, so
//! there is no oracle to check it against.

use crate::data_format::{DfError, R};
use crate::proc_base;
use crate::sniff::processor::{GameType, Proc, ProcBase, ProcContext, ProcFileObject};

pub struct ProcMoppUpdate {
    base: ProcBase,
}

impl ProcMoppUpdate {
    pub fn new() -> ProcMoppUpdate {
        let mut base = ProcBase::new(
            "Update MOPP code",
            &[GameType::Tes4, GameType::Fo3, GameType::Fnv],
            &["nif"],
        );
        // `fThreads := 1`: NifMopp.dll is not thread safe.
        base.threads = 1;
        ProcMoppUpdate { base }
    }
}

impl Default for ProcMoppUpdate {
    fn default() -> Self {
        ProcMoppUpdate::new()
    }
}

impl Proc for ProcMoppUpdate {
    proc_base!();

    // The frame holds a static text only: `on_show` reads no setting.

    fn on_start(&mut self) -> R<()> {
        // `OnStart`: `UnLoadMoppDll; LoadMoppDll;`
        load_mopp_dll()
    }

    fn process_file(&self, _: &mut ProcFileObject, _: &mut ProcContext) -> R<Vec<u8>> {
        // Unreachable: the run stops in `on_start` before any file, on the
        // released 64-bit build and without the DLL. Upstream would build
        // the vertex and triangle arrays of the strips shape, call
        // `GenerateMoppCodeWithSubshapes` and write `MOPP Data`, `Scale`
        // and `Origin`, and raise `MOPP generator returned 0 bytes` for an
        // empty result.
        Err(mopp_unavailable())
    }
}

/// `LoadMoppDll`: the refusal of the released 64-bit build, or the failure
/// to load the library the port does not ship.
fn load_mopp_dll() -> R<()> {
    // `{$IFDEF WIN64}` of `LoadMoppDll`.
    if cfg!(target_pointer_width = "64") {
        return Err(DfError::new(
            "You are running 64 bits version of Sniff. This operation works in 32 bits (x86) Sniff only.",
        ));
    }
    // The port has no `LoadLibrary` and ships no NifMopp.dll; upstream ends
    // this path with `Unable to load NifMopp.dll` whenever one of the four
    // entry points is missing, which it always is here.
    Err(mopp_unavailable())
}

fn mopp_unavailable() -> DfError {
    DfError::new("Unable to load NifMopp.dll")
}
