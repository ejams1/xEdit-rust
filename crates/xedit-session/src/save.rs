// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (SaveChanged), xEdit/xeInit.pas

//! The `files.save` command: a loaded plugin written as xEdit writes it,
//! and the per-game settings of `xeInit.pas` that the save path reads.
//!
//! `TfrmMain.SaveChanged` writes each file to `<name>.save.<timestamp>`,
//! drops the save when the CRC32 of the file did not change, moves the old
//! file to the backup folder and renames the save over it. Here the save
//! goes to a temporary file next to the target and is renamed over it
//! (design rule 4: atomic saves), the unchanged check and the backup are
//! kept, and `--dry-run` stops before the first byte is written.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::{FileImpl, ResetModified, SaveError};
use xedit_core::interface::globals::{
    GameMode, app_name, display_shorter_names, set_allow_esp_masters, set_allow_esp_masters_on_save,
    set_allow_internal_edit, set_always_save_onam, set_always_save_onam_force, set_app_name, set_can_sort_info,
    set_complex_file_file_id, set_display_load_order_form_id, set_display_shorter_names, set_enforce_all_masters,
    set_sort_sub_records, set_tool_name, set_vwd_as_quest_children, set_vwd_in_temporary,
};
use xedit_core::interface::{Element, File, FileState};

use crate::{CommandError, Registry, Session};

/// Port of the game specific settings of `xeInit.pas` that the save path
/// reads (`wbAllowESPMastersOnSave`, `wbAlwaysSaveOnam`, the VWD group
/// rules, `wbComplexFileFileID`, `wbEnforceAllMasters`) and the names upstream puts into its
/// messages (`wbAppName`, `wbToolName`). The dump setup of `xDump.dpr`
/// leaves them at their defaults, so they are applied after it.
pub fn apply_edit_settings(mode: GameMode) {
    set_tool_name("Edit");
    // `xeMainForm`: FormIDs are shown and edited as load order FormIDs, and
    // the subrecords of a record keep the order of the definition.
    set_display_load_order_form_id(true);
    set_sort_sub_records(true);
    // The default of `wbAllowInternalEdit`, which the games below override:
    // the load adds the required subrecords a record lacks.
    set_allow_internal_edit(true);
    let app = match mode {
        GameMode::gmTES3 => "TES3",
        GameMode::gmTES4 | GameMode::gmTES4R => "TES4",
        GameMode::gmFO3 => "FO3",
        GameMode::gmFNV => "FNV",
        GameMode::gmTES5 => "TES5",
        GameMode::gmEnderal => "Enderal",
        GameMode::gmTES5VR => "TES5VR",
        GameMode::gmSSE => "SSE",
        GameMode::gmEnderalSE => "EnderalSE",
        GameMode::gmFO4 => "FO4",
        GameMode::gmFO4VR => "FO4VR",
        GameMode::gmFO76 => "FO76",
        GameMode::gmSF1 => "SF1",
    };
    set_app_name(app);
    match mode {
        GameMode::gmFNV | GameMode::gmFO3 => {
            set_vwd_in_temporary(true);
            set_can_sort_info(true);
            set_allow_esp_masters(true);
            set_allow_esp_masters_on_save(true);
        }
        GameMode::gmTES3 => {
            set_allow_internal_edit(false);
            set_vwd_in_temporary(true);
            set_allow_esp_masters(true);
            set_allow_esp_masters_on_save(true);
        }
        GameMode::gmTES4 | GameMode::gmTES4R => {
            set_allow_internal_edit(false);
            set_can_sort_info(true);
            set_allow_esp_masters(true);
            set_allow_esp_masters_on_save(true);
        }
        GameMode::gmTES5 | GameMode::gmEnderal | GameMode::gmTES5VR | GameMode::gmSSE | GameMode::gmEnderalSE => {
            set_vwd_in_temporary(true);
            set_can_sort_info(true);
            set_allow_esp_masters(true);
            set_allow_esp_masters_on_save(true);
        }
        GameMode::gmFO4 | GameMode::gmFO4VR => {
            set_vwd_in_temporary(true);
            set_vwd_as_quest_children(true);
            set_always_save_onam(true);
            set_always_save_onam_force(true);
            set_allow_esp_masters(true);
            set_allow_esp_masters_on_save(true);
        }
        GameMode::gmFO76 => {
            set_vwd_in_temporary(true);
            set_vwd_as_quest_children(true);
            set_always_save_onam(true);
            set_always_save_onam_force(true);
        }
        GameMode::gmSF1 => {
            set_complex_file_file_id(true);
            set_enforce_all_masters(true);
            set_vwd_in_temporary(true);
            set_vwd_as_quest_children(true);
            set_always_save_onam(true);
            set_always_save_onam_force(true);
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesSaveRequest {
    /// Name of the loaded plugin to save; the only loaded plugin when omitted.
    pub file: Option<String>,
    /// Path to write to; the path the plugin was loaded from when omitted.
    pub output: Option<String>,
    /// Build the file and report, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
    /// Move an existing file at the output path to `<AppName>Edit Backups`
    /// next to it before it is replaced. Defaults to true, as upstream.
    #[serde(default = "default_true")]
    pub backup: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, JsonSchema)]
pub struct FilesSaveResponse {
    /// Name of the saved plugin.
    pub file: String,
    /// Path the file was (or would be) written to.
    pub output: String,
    /// Size of the saved file in bytes.
    pub bytes: u64,
    /// CRC32 of the saved bytes, eight hexadecimal digits.
    pub crc32: String,
    /// CRC32 of the file as it was loaded.
    pub loaded_crc32: String,
    /// Whether the saved bytes differ from the loaded file.
    pub changed: bool,
    /// Whether the file was written. False for a dry run, and for a save to
    /// the loaded path whose bytes did not change (upstream removes it).
    pub written: bool,
    /// Where the previous file went, when one existed and was backed up.
    pub backup: Option<String>,
}

/// The reset upstream applies with `wbResetModifiedOnSave`, which is on by
/// default.
const RESET_ON_SAVE: ResetModified = ResetModified::rmSetInternal;

fn files_save(session: &mut Session, request: FilesSaveRequest) -> Result<FilesSaveResponse, CommandError> {
    let file = session.file(request.file.as_deref())?;
    if file.get_file_states().contains(FileState::fsIsHardcoded) {
        return Err(CommandError::new(
            "save_refused",
            "the hardcoded records are not a file",
        ));
    }
    let loaded_path = PathBuf::from(file.file_name());
    let output = request.output.map_or_else(|| loaded_path.clone(), PathBuf::from);
    let to_loaded_path = same_path(&output, &loaded_path);
    let loaded_crc32 = file.crc32();
    let bytes = write_file(&file)?;
    let crc32 = file.crc32();
    let changed = crc32 != loaded_crc32;
    let mut response = FilesSaveResponse {
        file: file.get_name(),
        output: output.to_string_lossy().into_owned(),
        bytes: bytes.len() as u64,
        crc32: format!("{crc32:08X}"),
        loaded_crc32: format!("{loaded_crc32:08X}"),
        changed,
        written: false,
        backup: None,
    };
    if request.dry_run {
        return Ok(response);
    }
    // `SaveChanged`: a save over the loaded file that did not change it is
    // removed again.
    if to_loaded_path && !changed {
        return Ok(response);
    }
    if request.backup && output.is_file() {
        response.backup = Some(backup(&output)?);
    }
    write_atomically(&output, &bytes)?;
    response.written = true;
    Ok(response)
}

/// Builds the bytes of the file on a thread with a large stack, because the
/// record initialization resolves deeply through the definitions.
/// The names in the messages of the save are the GUI's (`xeMainForm` sets
/// `wbDisplayShorterNames`: `[TES4:00000000]` for the file header), as the
/// oracle reports them; the other commands keep the names of xDump.
fn write_file(file: &Arc<FileImpl>) -> Result<Vec<u8>, CommandError> {
    let file = file.clone();
    let shorter_names = display_shorter_names();
    set_display_shorter_names(true);
    let worker = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || file.write_to_bytes(RESET_ON_SAVE));
    let result = worker.map(|worker| worker.join());
    set_display_shorter_names(shorter_names);
    let result = result
        .map_err(|error| CommandError::new("internal", error.to_string()))?
        .map_err(|_| CommandError::new("internal", "the save thread panicked"))?;
    result.map_err(|error| match error {
        SaveError::Refused(message) => CommandError::new("save_refused", message),
        SaveError::Unsupported(message) => CommandError::new("unsupported", message),
        SaveError::Internal(message) => CommandError::new("internal", message),
    })
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Port of `DoBackupModule`: the file moves to `<AppName>Edit Backups`
/// next to it as `<name>.backup.<timestamp>`.
fn backup(target: &Path) -> Result<String, CommandError> {
    let dir = target
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join(format!("{}Edit Backups", app_name()));
    std::fs::create_dir_all(&dir).map_err(|error| CommandError::new("io", format!("{}: {error}", dir.display())))?;
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut path = dir.join(format!("{name}.backup.{}", timestamp()));
    let mut counter = 0;
    while path.exists() {
        counter += 1;
        path = dir.join(format!("{name}.backup.{}_{counter}", timestamp()));
    }
    std::fs::rename(target, &path).map_err(|error| CommandError::new("io", format!("{}: {error}", path.display())))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Writes `bytes` to a temporary file next to `target` and renames it over
/// `target`, so that a failure leaves the old file in place.
pub(crate) fn write_atomically(target: &Path, bytes: &[u8]) -> Result<(), CommandError> {
    let io = |path: &Path, error: std::io::Error| CommandError::new("io", format!("{}: {error}", path.display()));
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
    }
    let mut temp = target.as_os_str().to_owned();
    temp.push(format!(".{}.tmp", std::process::id()));
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, bytes).map_err(|error| io(&temp, error))?;
    if let Err(error) = std::fs::rename(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return Err(io(target, error));
    }
    Ok(())
}

/// The timestamp of a backup name, `yyyy_mm_dd_hh_nn_ss` as upstream formats
/// it, in UTC.
fn timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}_{m:02}_{d:02}_{:02}_{:02}_{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "files.save",
        "Write a loaded plugin to disk as xEdit saves it.",
        true,
        files_save,
    );
}

#[cfg(test)]
mod tests {
    use super::timestamp;

    #[test]
    fn timestamp_has_the_upstream_shape() {
        let stamp = timestamp();
        assert_eq!(stamp.len(), "yyyy_mm_dd_hh_nn_ss".len(), "{stamp}");
        assert!(stamp.starts_with("20"), "{stamp}");
    }
}
