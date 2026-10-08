// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavChangeFormIDClick,
// mniNavRenumberFormIDsFromClick, ShowChangeReferencedBy)

//! The FormID and module flag commands: `formids.change` (the "Change
//! FormID" of the navigation tree), `formids.renumber` ("Renumber FormIDs
//! from...", "Compact FormIDs for ESL" and "Renumber FormIDs and inject into
//! master..."), and `files.flags`, which sets the ESM, ESL, medium, update
//! (overlay), blueprint and localized flags of a module header as the
//! `SetIs...` methods of `TwbFile` do.
//!
//! The dialogs of upstream are parameters: the new FormID, whether later
//! overrides follow, the start FormID, the injection target and its two
//! questions. The referencing records come from the reference index
//! (`ReferencedBy` of the master, built on first use as the GUI builds it on
//! load); `formids.change` updates the
//! referencing records in editable files, as a user who keeps every record
//! the dialog offers, and `formids.renumber` updates them as upstream's
//! silent update does.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::file_flags::ModuleFlag;
use xedit_core::implementation::{FileImpl, MainRecordImpl};
use xedit_core::interface::{Element, File, FormID, MainRecord, MainRecordRef};

use crate::commands::parse_form_id;
use crate::{CommandError, Registry, Session};

/// The edit flag for the duration of a dry run: upstream's editability
/// checks (`GetIsEditable`) read `wbEditAllowed`, and a dry run reports what
/// the command would do with `--edit`. The flag goes back when this drops.
struct DryRunEdit(Option<bool>);

impl DryRunEdit {
    fn new(dry_run: bool) -> Self {
        let was = xedit_core::interface::globals::edit_allowed();
        if dry_run && !was {
            xedit_core::interface::globals::set_edit_allowed(true);
            DryRunEdit(Some(was))
        } else {
            DryRunEdit(None)
        }
    }
}

impl Drop for DryRunEdit {
    fn drop(&mut self) {
        if let Some(was) = self.0 {
            xedit_core::interface::globals::set_edit_allowed(was);
        }
    }
}

fn edit_failed(message: String) -> CommandError {
    CommandError::new("edit_failed", message)
}

pub(crate) fn record_impl(record: &MainRecordRef) -> Result<Arc<MainRecordImpl>, CommandError> {
    record
        .as_element_impl()
        .and_then(|element| element.main_record_impl())
        .ok_or_else(|| CommandError::new("unknown_record", format!("{} is not a main record", record.get_name())))
}

fn file_impl_of(record: &MainRecordImpl) -> Option<Arc<FileImpl>> {
    record
        .get_file()
        .and_then(|file| file.as_element_impl().and_then(|element| element.file_impl()))
}

/// Any loaded file by name, masters included.
fn any_file(name: &str) -> Result<Arc<FileImpl>, CommandError> {
    xedit_core::interface::files()
        .into_iter()
        .find(|file| file.get_name().eq_ignore_ascii_case(name))
        .and_then(|file| file.as_element_impl().and_then(|element| element.file_impl()))
        .ok_or_else(|| CommandError::new("unknown_file", format!("{name} is not loaded")))
}

/// Whether a record can be edited: `IsEditable` of the record and its file.
fn record_editable(record: &MainRecordImpl) -> bool {
    record.get_is_editable() && file_impl_of(record).is_some_and(|file| file.get_is_editable())
}

/// A record that was or would be updated.
#[derive(Serialize, JsonSchema, Clone)]
pub struct RecordRef {
    /// Load order FormID as eight hexadecimal digits.
    pub form_id: String,
    pub signature: String,
    pub editor_id: String,
    /// File that holds this version of the record.
    pub file: String,
}

pub(crate) fn record_ref(record: &MainRecordImpl) -> RecordRef {
    RecordRef {
        form_id: record.get_load_order_form_id().to_string(false),
        signature: record.get_signature().to_string(),
        editor_id: record.get_editor_id(),
        file: record.get_file().map(|file| file.get_name()).unwrap_or_default(),
    }
}

/// One FormID change.
#[derive(Serialize, JsonSchema)]
pub struct FormIdChange {
    /// Load order FormID before the change.
    pub old_form_id: String,
    /// Load order FormID after the change.
    pub new_form_id: String,
    pub signature: String,
    pub editor_id: String,
    /// Files whose version of the record took the new FormID (the record
    /// itself first, then its overrides).
    pub renumbered: Vec<String>,
    /// The records that refer to the record (`ReferencedBy` of its master).
    pub referenced_by: Vec<RecordRef>,
    /// How many of them now refer to the new FormID.
    pub references_updated: usize,
    /// The errors upstream logs and goes on after.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// Port of the update of `ShowChangeReferencedBy`: `CompareExchangeFormID`
/// on the referencing records. Silent (the renumbering) updates every
/// record when one of them is editable, as upstream; otherwise only the
/// editable ones are updated, as a user who keeps all records the dialog
/// offers.
fn update_referencing(
    old: FormID,
    new: FormID,
    referenced_by: &[Arc<MainRecordImpl>],
    silent: bool,
    errors: &mut Vec<String>,
) -> usize {
    if !referenced_by.iter().any(|record| record_editable(record)) {
        return 0;
    }
    let mut count = 0;
    for record in referenced_by {
        if !silent && !record_editable(record) {
            continue;
        }
        match record.compare_exchange_form_id(old, new) {
            Ok(true) => count += 1,
            Ok(false) => {}
            Err(error) => errors.push(format!(
                "Error updating FormID for {} - {}: {error}",
                record.get_name(),
                record.get_file().map(|file| file.get_name()).unwrap_or_default()
            )),
        }
    }
    count
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormIdsChangeRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// The new load order FormID as hexadecimal digits. When omitted, the
    /// next free FormID of the record's file (`NewFormID`), or of
    /// `target_file`.
    pub new_form_id: Option<String>,
    /// Plugin whose version of the record changes; the last loaded plugin
    /// when omitted.
    pub file: Option<String>,
    /// Take the next free FormID of this file, the record's file or one of
    /// its masters, as upstream's "renumber to destination file" does; the
    /// referencing records are then updated silently.
    pub target_file: Option<String>,
    /// Change the later overrides too (upstream asks "has later overrides,
    /// update them too?").
    #[serde(default)]
    pub overrides: bool,
    /// Report what would change, but change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct FormIdsChangeResponse {
    #[serde(flatten)]
    pub change: FormIdChange,
    /// The files that became masters of the record's file because the new
    /// FormID belongs to them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub masters_added: Vec<String>,
    /// False for a dry run.
    pub changed: bool,
}

fn formids_change(session: &mut Session, request: FormIdsChangeRequest) -> Result<FormIdsChangeResponse, CommandError> {
    let _edit = DryRunEdit::new(request.dry_run);
    let record = session.record(&request.form_id, request.file.as_deref())?;
    let record = record_impl(&record)?;
    if record.get_signature().to_string() == "TES4" || !record_editable(&record) {
        return Err(CommandError::new(
            "not_editable",
            format!("{} can not be edited", record.get_name()),
        ));
    }
    let file = file_impl_of(&record).ok_or_else(|| CommandError::new("unknown_file", "the record has no file"))?;
    let target = match &request.target_file {
        Some(name) => {
            let target = any_file(name)?;
            let allowed =
                Arc::ptr_eq(&target, &file) || file.masters().iter().any(|master| Arc::ptr_eq(master, &target));
            if !allowed {
                return Err(CommandError::new(
                    "invalid_params",
                    format!("{name} is neither {} nor one of its masters", file.get_name()),
                ));
            }
            Some(target)
        }
        None => None,
    };
    let old = record.get_load_order_form_id();
    let new = match (&request.new_form_id, &target) {
        (Some(text), _) => {
            let new = parse_form_id(text)?;
            if new.is_null() {
                return Err(CommandError::new("invalid_params", "00000000 is not a valid FormID"));
            }
            if new.is_player() {
                return Err(CommandError::new("invalid_params", "00000014 is not a valid FormID"));
            }
            new
        }
        (None, target) => {
            let target = target.clone().unwrap_or_else(|| file.clone());
            if target.get_load_order_file_id() == old.file_id() && request.target_file.is_some() {
                // Upstream skips a record that is in the target file already.
                return Ok(FormIdsChangeResponse {
                    change: FormIdChange {
                        old_form_id: old.to_string(false),
                        new_form_id: old.to_string(false),
                        signature: record.get_signature().to_string(),
                        editor_id: record.get_editor_id(),
                        renumbered: Vec::new(),
                        referenced_by: Vec::new(),
                        references_updated: 0,
                        errors: Vec::new(),
                    },
                    masters_added: Vec::new(),
                    changed: false,
                });
            }
            if request.dry_run {
                // `NewFormID` moves the next object ID: a dry run only reads it.
                FormID::from_cardinal(target.get_next_object_id()).change_file_id(target.get_load_order_file_id())
            } else {
                let new = target.new_form_id().map_err(edit_failed)?;
                target
                    .file_form_id_to_load_order_form_id(new, true)
                    .map_err(edit_failed)?
            }
        }
    };
    // The file must see the file of the new FormID; a file before it in
    // the load order with that FileID becomes its master
    // (`AddRequiredMaster`, without the question).
    let mut masters_added = Vec::new();
    if let Err(error) = File::load_order_form_id_to_file_form_id(&*file, new, true) {
        let new_master = xedit_core::interface::files()
            .into_iter()
            .filter(|candidate| candidate.get_load_order() < file.get_load_order())
            .find(|candidate| candidate.get_load_order_file_id() == new.file_id())
            .ok_or_else(|| edit_failed(error))?;
        masters_added.push(new_master.get_name());
        if !request.dry_run {
            file.add_masters_if_missing(&masters_added, true, false)
                .map_err(edit_failed)?;
            file.sort_masters().map_err(edit_failed)?;
        }
    }

    session.ensure_refs()?;
    let master = record.master_or_self_impl();
    let referenced_by = master.referenced_by();
    // The overrides that follow: all of them for the master, the later ones
    // for an override that is not the last.
    let overrides = master.overrides();
    let follow: Vec<Arc<MainRecordImpl>> = if request.overrides && !overrides.is_empty() {
        let position = overrides.iter().position(|candidate| Arc::ptr_eq(candidate, &record));
        match position {
            Some(index) if index + 1 < overrides.len() => overrides[index..].to_vec(),
            Some(_) => Vec::new(),
            None => overrides.clone(),
        }
    } else {
        Vec::new()
    };
    let mut change = FormIdChange {
        old_form_id: old.to_string(false),
        new_form_id: new.to_string(false),
        signature: record.get_signature().to_string(),
        editor_id: record.get_editor_id(),
        renumbered: Vec::new(),
        referenced_by: referenced_by.iter().map(|record| record_ref(record)).collect(),
        references_updated: 0,
        errors: Vec::new(),
    };
    if request.dry_run || new == old {
        return Ok(FormIdsChangeResponse {
            change,
            masters_added,
            changed: false,
        });
    }
    record.set_load_order_form_id(new).map_err(edit_failed)?;
    change.renumbered.push(file.get_name());
    for override_record in &follow {
        if override_record.get_load_order_form_id() == new {
            continue;
        }
        let name = override_record
            .get_file()
            .map(|file| file.get_name())
            .unwrap_or_default();
        match override_record.set_load_order_form_id(new) {
            Ok(()) => change.renumbered.push(name),
            Err(error) => change.errors.push(format!(
                "Error renumbering {}: {error}",
                override_record.get_full_path()
            )),
        }
    }
    change.references_updated = update_referencing(old, new, &referenced_by, target.is_some(), &mut change.errors);
    Ok(FormIdsChangeResponse {
        change,
        masters_added,
        changed: true,
    })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormIdsRenumberRequest {
    /// Name of the loaded plugin whose new records take new FormIDs; the
    /// only loaded plugin when omitted.
    pub file: Option<String>,
    /// The first object ID in hexadecimal (six digits, three for a light
    /// target); the next object ID of the target when omitted.
    pub start: Option<String>,
    /// Compact the FormIDs into the light range from `000800` (upstream's
    /// "Compact FormIDs for ESL").
    #[serde(default)]
    pub compact: bool,
    /// Give the records FormIDs of this master instead (upstream's
    /// "Renumber FormIDs and inject into master...").
    pub inject_into: Option<String>,
    /// With `inject_into`: keep the object IDs where the master has them
    /// free.
    #[serde(default)]
    pub preserve_object_ids: bool,
    /// With `preserve_object_ids`: stop when an object ID can not be kept.
    #[serde(default)]
    pub all_or_nothing: bool,
    /// Report the plan, but change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct FormIdsRenumberResponse {
    pub file: String,
    /// The file the new FormIDs belong to.
    pub target: String,
    /// The changes, in the order they run.
    pub changes: Vec<FormIdChange>,
    /// Signatures of the records that change, as the confirmation lists them.
    pub signatures: Vec<String>,
    /// The object IDs kept with `preserve_object_ids`.
    pub preserved: usize,
    /// The next object ID written to the target's header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_object_id: Option<String>,
    /// False for a dry run and when there is nothing to do.
    pub changed: bool,
}

/// The plan of `mniNavRenumberFormIDsFromClick.Prepare`: the records with
/// their new FormIDs, the highest FormID and the preserved count.
struct RenumberPlan {
    records: Vec<Arc<MainRecordImpl>>,
    targets: Vec<FormID>,
    high: FormID,
    preserved: usize,
}

fn plan_renumber(
    source: &Arc<FileImpl>,
    target: &Arc<FileImpl>,
    request: &FormIdsRenumberRequest,
) -> Result<Option<RenumberPlan>, CommandError> {
    let invalid = |message: String| CommandError::new("invalid_params", message);
    if target.get_is_update() {
        return Err(invalid(format!(
            "\"{}\" is an update module and can't own any records.",
            target.get_name()
        )));
    }
    let lowest: u32 = if target.get_allow_hardcoded_range_use() {
        1
    } else {
        0x800
    };
    let same = Arc::ptr_eq(source, target);
    let all_or_nothing = request.preserve_object_ids && request.all_or_nothing;
    let mut start = if all_or_nothing || request.compact {
        FormID::from_cardinal(lowest)
    } else {
        let target_is_light = target.get_is_light() || target.get_load_order_file_id().is_light_slot();
        match &request.start {
            Some(text) => {
                let start = FormID::from_str(text.trim()).unwrap_or(FormID::from_cardinal(0));
                if start.file_id().full_slot() != 0
                    || start.to_cardinal() < lowest
                    || (target_is_light && start.object_id() > 0xFFF)
                {
                    return Err(invalid(format!("\"{text}\" is not a valid start FormID.")));
                }
                start
            }
            None => {
                let mut next = target.get_next_object_id() & 0xFF_FFFF;
                if target_is_light {
                    next &= 0xFFF;
                }
                FormID::from_cardinal(next.max(lowest))
            }
        }
    };
    // The new records of the source, in FormID order.
    let records: Vec<Arc<MainRecordImpl>> = source
        .records()
        .into_iter()
        .filter(|record| record.get_load_order_form_id().file_id() == source.get_load_order_file_id())
        .collect();
    if records.is_empty() {
        return Ok(None);
    }
    start = start.change_file_id(target.get_load_order_file_id());
    let mut high = start;
    let end = if request.compact || (!same && target.get_is_light()) {
        FormID::from_cardinal(0xFFF).change_file_id(target.get_load_order_file_id())
    } else if !same {
        FormID::from_cardinal(0xFF_FFFF).change_file_id(target.get_load_order_file_id())
    } else {
        let end = FormID::from_cardinal(start.to_cardinal() + records.len() as u32);
        high = end;
        end
    };
    let mut taken = vec![false; records.len()];
    if same {
        for record in &records {
            let old = record.get_load_order_form_id();
            if old >= start && old <= end {
                let index = (old.to_cardinal() - start.to_cardinal()) as usize;
                if index >= taken.len() {
                    taken.resize(index + 1, false);
                }
                taken[index] = true;
            }
        }
    }
    let mut planned: Vec<Arc<MainRecordImpl>> = Vec::new();
    let mut targets: Vec<FormID> = Vec::new();
    let mut preserved = 0;
    let mut any_delayed = false;
    let mut j: u32 = 0;
    let too_many = || invalid("The file contains too many new records for this operation.".to_owned());
    for record in &records {
        let old = record.get_load_order_form_id();
        let new = if same {
            if old >= start && old <= end {
                continue;
            }
            while (j as usize) < taken.len() && taken[j as usize] {
                j += 1;
            }
            let new = FormID::from_cardinal(start.to_cardinal() + j);
            j += 1;
            new
        } else {
            let mut new = FormID::null();
            loop {
                if request.preserve_object_ids {
                    if new.is_null() {
                        new = old.change_file_id(target.get_load_order_file_id());
                    } else if all_or_nothing {
                        let holder = target
                            .contained_record_by_load_order_form_id_injected(new)
                            .map(|holder| holder.get_name())
                            .unwrap_or_default();
                        return Err(CommandError::new(
                            "edit_failed",
                            format!(
                                "The FormID [{}] which should be assigned to: {} is already in use by: {holder}. Operation aborted.",
                                new.to_string(true),
                                record.get_name()
                            ),
                        ));
                    } else {
                        new = FormID::null();
                        any_delayed = true;
                        break;
                    }
                } else {
                    new = FormID::from_cardinal(start.to_cardinal() + j);
                    j += 1;
                }
                if target.contained_record_by_load_order_form_id_injected(new).is_none() {
                    break;
                }
            }
            new
        };
        if new > end {
            return Err(too_many());
        }
        if new > high {
            high = new;
        }
        if new == old {
            continue;
        }
        if request.preserve_object_ids && !new.is_null() {
            preserved += 1;
        }
        planned.push(record.clone());
        targets.push(new);
    }
    if any_delayed {
        for target_form_id in targets.iter_mut() {
            if !target_form_id.is_null() {
                continue;
            }
            let mut new;
            loop {
                new = FormID::from_cardinal(start.to_cardinal() + j);
                j += 1;
                if target.contained_record_by_load_order_form_id_injected(new).is_none() {
                    break;
                }
            }
            if new > end {
                return Err(too_many());
            }
            if new > high {
                high = new;
            }
            *target_form_id = new;
        }
    }
    if planned.is_empty() {
        return Ok(None);
    }
    Ok(Some(RenumberPlan {
        records: planned,
        targets,
        high,
        preserved,
    }))
}

fn formids_renumber(
    session: &mut Session,
    request: FormIdsRenumberRequest,
) -> Result<FormIdsRenumberResponse, CommandError> {
    let _edit = DryRunEdit::new(request.dry_run);
    let source = session.file(request.file.as_deref())?;
    if !source.get_is_editable() {
        return Err(CommandError::new(
            "not_editable",
            format!("File \"{}\" is not editable", source.get_name()),
        ));
    }
    let target = match &request.inject_into {
        Some(name) => {
            let target = any_file(name)?;
            if !source.masters().iter().any(|master| Arc::ptr_eq(master, &target)) {
                return Err(CommandError::new(
                    "invalid_params",
                    format!("{name} is not a master of {}", source.get_name()),
                ));
            }
            target
        }
        None => source.clone(),
    };
    let mut response = FormIdsRenumberResponse {
        file: source.get_name(),
        target: target.get_name(),
        changes: Vec::new(),
        signatures: Vec::new(),
        preserved: 0,
        next_object_id: None,
        changed: false,
    };
    let Some(plan) = plan_renumber(&source, &target, &request)? else {
        return Ok(response);
    };
    response.preserved = plan.preserved;
    let mut signatures: Vec<String> = plan
        .records
        .iter()
        .map(|record| record.get_signature().to_string())
        .collect();
    signatures.sort();
    signatures.dedup();
    response.signatures = signatures;

    session.ensure_refs()?;
    for (record, new) in plan.records.iter().zip(&plan.targets) {
        let old = record.get_load_order_form_id();
        // Read before the change, as upstream: the change moves the list.
        let referenced_by = record.master_or_self_impl().referenced_by();
        let mut change = FormIdChange {
            old_form_id: old.to_string(false),
            new_form_id: new.to_string(false),
            signature: record.get_signature().to_string(),
            editor_id: record.get_editor_id(),
            renumbered: Vec::new(),
            referenced_by: referenced_by.iter().map(|record| record_ref(record)).collect(),
            references_updated: 0,
            errors: Vec::new(),
        };
        if !request.dry_run {
            let overrides = record.overrides();
            match record.set_load_order_form_id(*new) {
                Ok(()) => {
                    change.renumbered.push(source.get_name());
                    for override_record in &overrides {
                        let name = override_record
                            .get_file()
                            .map(|file| file.get_name())
                            .unwrap_or_default();
                        match override_record.set_load_order_form_id(*new) {
                            Ok(()) => change.renumbered.push(name),
                            Err(error) => change.errors.push(format!("Error: {error}")),
                        }
                    }
                    if !referenced_by.is_empty() {
                        change.references_updated =
                            update_referencing(old, *new, &referenced_by, true, &mut change.errors);
                    }
                }
                Err(error) => change.errors.push(format!("Error: {error}")),
            }
        }
        response.changes.push(change);
    }
    if !request.dry_run {
        // `UpdateNextObjectID`.
        if target.get_is_editable() {
            let next = plan.high.object_id() + 1;
            target.set_next_object_id(next).map_err(edit_failed)?;
            response.next_object_id = Some(format!("{next:06X}"));
        }
        response.changed = true;
    }
    Ok(response)
}

/// The module flags of a file header.
#[derive(Serialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
pub struct ModuleFlags {
    pub esm: bool,
    /// The light (ESL, small) flag.
    pub light: bool,
    /// The medium flag (Starfield).
    pub medium: bool,
    /// The update (overlay) flag (Starfield).
    pub update: bool,
    /// The blueprint flag (Starfield).
    pub blueprint: bool,
    pub localized: bool,
}

fn module_flags(file: &FileImpl) -> ModuleFlags {
    ModuleFlags {
        esm: file.module_flag(ModuleFlag::Esm),
        light: file.module_flag(ModuleFlag::Light),
        medium: file.module_flag(ModuleFlag::Medium),
        update: file.module_flag(ModuleFlag::Update),
        blueprint: file.module_flag(ModuleFlag::Blueprint),
        localized: file.module_flag(ModuleFlag::Localized),
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesFlagsRequest {
    /// Name of the loaded plugin; the only loaded plugin when omitted.
    pub file: Option<String>,
    pub esm: Option<bool>,
    /// The light (ESL) flag, where the game has it.
    pub light: Option<bool>,
    /// The medium flag, where the game has it (Starfield). Setting it clears
    /// the light and update flags.
    pub medium: Option<bool>,
    /// The update (overlay) flag, where the game has it (Starfield). Setting
    /// it clears the light and medium flags.
    pub update: Option<bool>,
    /// The blueprint flag, where the game has it (Starfield).
    pub blueprint: Option<bool>,
    pub localized: Option<bool>,
    /// Report the flags the change would give, but change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct FilesFlagsResponse {
    pub file: String,
    pub before: ModuleFlags,
    pub after: ModuleFlags,
    /// Whether the header flags differ after the call.
    pub changed: bool,
    /// The flags this game has; a flag it lacks is never set.
    pub supported: Vec<String>,
    /// Whether every new record of the file has an object ID up to `000FFF`,
    /// so that the file saves with the light flag.
    pub light_compatible: bool,
    /// Whether every new record has an object ID up to `00FFFF` (medium flag).
    pub medium_compatible: bool,
    /// Whether the file has no new records (update flag).
    pub update_compatible: bool,
}

const FLAG_ORDER: [ModuleFlag; 6] = [
    ModuleFlag::Esm,
    ModuleFlag::Localized,
    ModuleFlag::Blueprint,
    ModuleFlag::Update,
    ModuleFlag::Medium,
    ModuleFlag::Light,
];

fn flag_name(flag: ModuleFlag) -> &'static str {
    match flag {
        ModuleFlag::Esm => "esm",
        ModuleFlag::Light => "light",
        ModuleFlag::Medium => "medium",
        ModuleFlag::Update => "update",
        ModuleFlag::Blueprint => "blueprint",
        ModuleFlag::Localized => "localized",
    }
}

fn files_flags(session: &mut Session, request: FilesFlagsRequest) -> Result<FilesFlagsResponse, CommandError> {
    let _edit = DryRunEdit::new(request.dry_run);
    let file = session.file(request.file.as_deref())?;
    let before = module_flags(&file);
    let wanted = |flag: ModuleFlag| match flag {
        ModuleFlag::Esm => request.esm,
        ModuleFlag::Light => request.light,
        ModuleFlag::Medium => request.medium,
        ModuleFlag::Update => request.update,
        ModuleFlag::Blueprint => request.blueprint,
        ModuleFlag::Localized => request.localized,
    };
    let any_change = FLAG_ORDER
        .iter()
        .any(|flag| wanted(*flag).is_some_and(|value| value != file.module_flag(*flag)));
    let after = if request.dry_run {
        if any_change && !file.is_element_editable() {
            return Err(edit_failed(format!("File \"{}\" is not editable", file.file_name())));
        }
        let mut flags = file.header().map(|header| header.mr_struct().flags).unwrap_or_default();
        if !file.get_is_not_plugin() {
            for flag in FLAG_ORDER {
                let Some(value) = wanted(flag) else { continue };
                if !FileImpl::module_flag_supported(flag) {
                    continue;
                }
                match flag {
                    ModuleFlag::Esm => flags.set_esm(value),
                    ModuleFlag::Light => flags.set_light(value),
                    ModuleFlag::Medium => flags.set_medium(value),
                    ModuleFlag::Update => flags.set_update(value),
                    ModuleFlag::Blueprint => flags.set_blueprint(value),
                    ModuleFlag::Localized => flags.set_localized(value),
                }
            }
        }
        ModuleFlags {
            esm: flags.is_esm(),
            light: flags.is_light(),
            medium: flags.is_medium(),
            update: flags.is_update(),
            blueprint: flags.is_blueprint(),
            localized: flags.is_localized(),
        }
    } else {
        for flag in FLAG_ORDER {
            if let Some(value) = wanted(flag) {
                file.set_module_flag(flag, value).map_err(edit_failed)?;
            }
        }
        module_flags(&file)
    };
    let own = file.get_file_file_id();
    let own_records: Vec<u32> = file
        .records()
        .iter()
        .map(|record| record.get_fixed_form_id())
        .filter(|form_id| form_id.file_id() == own)
        .map(FormID::to_cardinal)
        .collect();
    Ok(FilesFlagsResponse {
        file: file.get_name(),
        changed: after != before,
        before,
        after,
        supported: FLAG_ORDER
            .iter()
            .filter(|flag| FileImpl::module_flag_supported(**flag))
            .map(|flag| flag_name(*flag).to_owned())
            .collect(),
        light_compatible: own_records.iter().all(|form_id| form_id & 0x00FF_F000 == 0),
        medium_compatible: own_records.iter().all(|form_id| form_id & 0x00FF_0000 == 0),
        update_compatible: own_records.is_empty(),
    })
}

/// Adds the FormID and module flag commands to the registry.
pub fn register(registry: &mut Registry) {
    registry.register(
        "formids.change",
        "Change the FormID of a record and update the records that refer to it.",
        true,
        formids_change,
    );
    registry.register(
        "formids.renumber",
        "Renumber the new records of a plugin from a start FormID, compact them for ESL, or inject them into a master.",
        true,
        formids_renumber,
    );
    registry.register(
        "files.flags",
        "Read or set the ESM, ESL, medium, update, blueprint and localized flags of a plugin header.",
        true,
        files_flags,
    );
}
