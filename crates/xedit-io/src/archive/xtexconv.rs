// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: BSArch/frmMain.pas (DDS2XBOX)

//! `DDS2XBOX`: a DDS file converted to the XBox 360 format by
//! `xtexconv.exe`, Microsoft's Xenon texture tool, which stands next to
//! the program (the GUI's `TAssetsPacker.ConvertDDS2XBOX`). The tool is
//! run as `xtexconv.exe -xbox "<file>"` on a temporary copy, the copy is
//! read back and deleted, and a failure gives upstream's messages. Neither
//! the release archive nor this repository ships the tool: the packer only
//! reaches this when the file is there, as the GUI does
//! (`FileExists(ExtractFilePath(ParamStr(0)) + 'xtexconv.exe')`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// `TPath.GetTempPath + TPath.GetGUIDFileName + '.dds'`: a file of the
/// temp folder that nothing else uses.
fn temp_file() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("xtexconv-{}-{n}.dds", std::process::id()))
}

/// The `DDS2XBOX` function: the tool converts the copy and it is read
/// back. The error is the message of the exception upstream raises.
pub fn dds2xbox(tool: &Path, data: &[u8]) -> Result<Vec<u8>, String> {
    let temp = temp_file();
    if std::fs::write(&temp, data).is_err() {
        return Err(format!(
            "Can't create temp file for DDS XBox convertion: {}",
            temp.display()
        ));
    }
    let result = (|| -> Result<Vec<u8>, String> {
        // `ShellExecuteEx` with `SW_HIDE` and the temp folder as the
        // working folder; a tool that cannot start is upstream's
        // "Unable to start xtexconv.exe".
        let status = Command::new(tool)
            .arg("-xbox")
            .arg(&temp)
            .current_dir(temp.parent().unwrap_or(Path::new(".")))
            .status()
            .map_err(|_| "Unable to start xtexconv.exe".to_owned())?;
        if !status.success() {
            // `ErrCode.ToHexString(8)`: eight upper-case hex digits.
            let code = status.code().unwrap_or(0) as u32;
            return Err(format!("xtexconv.exe error code {code:08X}"));
        }
        std::fs::read(&temp).map_err(|error| error.to_string())
    })();
    let _ = std::fs::remove_file(&temp);
    result
}
