// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavCopyIdleClick)

//! `records.copy_idles`: "Copy Idles" of the games up to Fallout: New
//! Vegas. The `IDLE` records of the loaded files are grouped by the folder
//! of their model path; the chosen folder is copied as new records into a
//! plugin, the model path of each copy takes the new prefix, and the copies
//! that referred to the old records now refer to the copies.

use std::collections::HashMap;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::MainRecordImpl;
use xedit_core::implementation::copy::{add_required_masters_of, copy_into_file, missing_masters, required_masters};
use xedit_core::interface::globals::GameMode;
use xedit_core::interface::{Container, CopyArgs, Element, MainRecord, MainRecordRef};

use crate::commands::{RecordSummary, summary_of};
use crate::edit::with_edit_allowed;
use crate::{CommandError, Registry, Session};

/// Delphi `ExtractFileExt`: the extension from the last dot after the last
/// path delimiter, with the dot.
fn file_ext(path: &str) -> &str {
    let name_start = path.rfind(['\\', '/']).map_or(0, |index| index + 1);
    match path[name_start..].rfind('.') {
        Some(index) => &path[name_start + index..],
        None => "",
    }
}

/// Delphi `ExcludeTrailingPathDelimiter`.
fn exclude_trailing_path_delimiter(path: &str) -> &str {
    path.trim_end_matches(['\\', '/'])
}

/// The model prefix of the release: the lower case model path, its folder
/// when it has an extension, without the trailing delimiter.
fn model_prefix(model: &str) -> String {
    let model = model.trim().to_lowercase();
    let prefix = if file_ext(&model).is_empty() {
        model
    } else {
        xedit_core::delphi::extract_file_path(&model)
    };
    exclude_trailing_path_delimiter(&prefix).to_owned()
}

/// `records.copy_idles`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CopyIdlesRequest {
    /// The model prefix (the folder of the `MODL` path) of the idles to
    /// copy, as the file selection lists it.
    pub path: String,
    /// The new model prefix (the dialog's InputQuery), which must differ.
    pub new_path: String,
    /// Plugin to copy the idle records into.
    pub to: String,
    /// Report the copies, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// One copied idle.
#[derive(Serialize, JsonSchema)]
pub struct CopiedIdle {
    pub source: RecordSummary,
    pub copy: RecordSummary,
    /// The model path before and after the prefix change.
    pub old_model: String,
    pub new_model: String,
}

/// `records.copy_idles`: the response.
#[derive(Serialize, JsonSchema)]
pub struct CopyIdlesResponse {
    /// The model prefix copied.
    pub path: String,
    /// The new model prefix.
    pub new_path: String,
    pub idles: Vec<CopiedIdle>,
    /// The plugin copied into.
    pub file: String,
    pub dry_run: bool,
}

/// `records.copy_idles`: the "Copy Idles" item of `mniNavCopyIdleClick`.
/// The `IDLE` records of the loaded files are grouped by the model prefix
/// of their winning override; those of `path` are copied as new records
/// (`CopyInto` with AsNew and DeepCopy), each copy's `MODL\MODL` takes
/// `new_path` in place of `path`, and every copy's references to the old
/// records are changed to the copies (`CompareExchangeFormID`).
fn records_copy_idles(session: &mut Session, request: CopyIdlesRequest) -> Result<CopyIdlesResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("records.copy_idles")?;
    if !matches!(
        session.mode()?,
        GameMode::gmTES3 | GameMode::gmTES4 | GameMode::gmTES4R | GameMode::gmFO3 | GameMode::gmFNV
    ) {
        return Err(CommandError::new(
            "invalid_params",
            "Copy Idles is offered for Morrowind up to Fallout: New Vegas only",
        ));
    }
    let prefix = model_prefix(&request.path);
    if prefix.eq_ignore_ascii_case(request.new_path.trim()) {
        return Err(CommandError::new(
            "invalid_params",
            "the new model prefix has to differ from the old one",
        ));
    }
    let target = session.file(Some(&request.to))?;
    if !with_edit_allowed(|| target.get_is_editable()) {
        return Err(CommandError::new(
            "not_editable",
            format!("File \"{}\" is not editable", target.get_name()),
        ));
    }
    // The winning overrides of the IDLE records of every loaded file, each
    // once (`Tag` of the release), by their model prefix.
    let mut groups: HashMap<String, Vec<Arc<MainRecordImpl>>> = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for file in &session.files {
        for record in file.records() {
            if record.get_signature().to_string() != "IDLE" {
                continue;
            }
            let record: MainRecordRef = record as MainRecordRef;
            let winning = record.get_winning_override();
            if !seen.insert(winning.get_element_id()) {
                continue;
            }
            let Some(winning) = crate::formids::record_impl(&winning).ok() else {
                continue;
            };
            let model = winning.get_element_edit_value("MODL\\MODL");
            groups.entry(model_prefix(&model)).or_default().push(winning);
        }
    }
    let Some(idles) = groups.get(&prefix) else {
        return Err(CommandError::new(
            "not_found",
            format!("no idle has the model prefix \"{prefix}\""),
        ));
    };
    let mut copied = Vec::new();
    if request.dry_run {
        for record in idles {
            let old_model = record.get_element_edit_value("MODL\\MODL");
            let remainder = old_model
                .trim()
                .to_lowercase()
                .strip_prefix(&prefix)
                .map(str::to_owned)
                .unwrap_or_default();
            copied.push(CopiedIdle {
                source: summary_of(&(record.clone() as MainRecordRef)),
                copy: summary_of(&(record.clone() as MainRecordRef)),
                old_model,
                new_model: format!("{}{remainder}", request.new_path.trim()),
            });
        }
    } else {
        // The copies first, then the model paths and the FormIDs, as the
        // release does (the old FormIDs are read before any changes).
        let mut pairs: Vec<(Arc<MainRecordImpl>, MainRecordRef)> = Vec::new();
        for record in idles {
            let source: xedit_core::interface::ElementRef = record.clone();
            let required = required_masters(&source, true);
            missing_masters(&required, &target).map_err(|error| CommandError::new("edit_failed", error))?;
            add_required_masters_of(&required, &target).map_err(|error| CommandError::new("edit_failed", error))?;
            let args = CopyArgs {
                as_new: true,
                deep_copy: true,
                ..CopyArgs::default()
            };
            let copy = copy_into_file(&source, &target, &args)
                .map_err(|error| CommandError::new("edit_failed", error))?
                .and_then(|copy| copy.into_main_record())
                .ok_or_else(|| CommandError::new("edit_failed", "the idle was not copied"))?;
            pairs.push((record.clone(), copy));
        }
        let form_ids: Vec<(u32, u32)> = pairs
            .iter()
            .map(|(old, new)| {
                (
                    old.get_load_order_form_id().to_cardinal(),
                    new.get_load_order_form_id().to_cardinal(),
                )
            })
            .collect();
        for (old, new) in &pairs {
            let old_model = old.get_element_edit_value("MODL\\MODL");
            let remainder = old_model
                .trim()
                .to_lowercase()
                .strip_prefix(&prefix)
                .map(str::to_owned)
                .unwrap_or_default();
            let new_model = format!("{}{remainder}", request.new_path.trim());
            let new_impl = crate::formids::record_impl(new)?;
            new_impl
                .set_element_edit_value("MODL\\MODL", &new_model)
                .map_err(|error| CommandError::new("edit_failed", error))?;
            copied.push(CopiedIdle {
                source: summary_of(&(old.clone() as MainRecordRef)),
                copy: summary_of(new),
                old_model,
                new_model,
            });
        }
        for (old, new) in &pairs {
            let new_impl = crate::formids::record_impl(new)?;
            for (old_id, new_id) in &form_ids {
                new_impl
                    .compare_exchange_form_id(
                        xedit_core::interface::FormID::from_cardinal(*old_id),
                        xedit_core::interface::FormID::from_cardinal(*new_id),
                    )
                    .map_err(|error| CommandError::new("edit_failed", error))?;
            }
            let _ = old;
        }
    }
    Ok(CopyIdlesResponse {
        path: prefix,
        new_path: request.new_path,
        idles: copied,
        file: target.get_name(),
        dry_run: request.dry_run,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "records.copy_idles",
        "Copy the IDLE records of a model folder as new records, change their model prefix and their references (Copy Idles).",
        true,
        records_copy_idles,
    );
}
