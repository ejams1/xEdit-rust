// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavFilterForCleaningClick,
// mniNavUndeleteAndDisableReferencesClick,
// mniNavRemoveIdenticalToMasterClick, the quick clean steps of
// tmrGeneratorTimer)

//! `files.clean`: "Remove Identical to Master records" (ITM), "Undelete and
//! Disable References" (UDR) and the quick auto clean mode
//! (`-quickautoclean`) on one loaded plugin. The cleaning itself is
//! `xedit_analysis::cleaning`.
//!
//! The quick auto clean mode runs a pass (the filter for cleaning, the UDR,
//! the ITM removal) and saves the plugin when the pass changed it; then it
//! runs a second pass and saves again when that changed it, and a third
//! pass, whose changes upstream does not save (`tmrGeneratorTimer`). The
//! mode loads the plugins with its own settings (`xeInit.pas`: the full
//! record definitions, the PNAM fill), which the CLI applies for
//! `xedit clean --quick` (`commands::set_quick_clean_on_load`); see
//! [`quick_auto_clean`].

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_analysis::cleaning::{self, CleanReport, CleanedElement, QuickCleanPass, UdrSettings};
use xedit_analysis::filter::{FilterOptions, FilterReport, NavTree, apply_filter};
use xedit_core::implementation::{ElementImpl, ElementState, FileImpl};
use xedit_core::interface::globals::{
    can_sort_info, display_shorter_names, edit_allowed, fill_pnam, set_display_shorter_names, set_edit_allowed,
    simple_records,
};
use xedit_core::interface::{Element, File};

use crate::save::{FilesSaveResponse, save_file};
use crate::{CommandError, Registry, Session};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CleanRequest {
    /// Name of the loaded plugin to clean; the only loaded plugin when
    /// omitted. Its masters must be loaded with it.
    pub file: Option<String>,
    /// Remove the records that are identical to their master, and the
    /// groups left empty ("Remove Identical to Master records").
    #[serde(default)]
    pub itm: bool,
    /// Undelete the deleted references and disable them ("Undelete and
    /// Disable References").
    #[serde(default)]
    pub udr: bool,
    /// The quick auto clean mode (`-quickautoclean`): UDR and ITM, saved,
    /// and again while a pass changes the plugin. xEdit loads the plugins
    /// for it with the full record definitions and the PNAM fill;
    /// `xedit clean --quick` does too, and a session loaded otherwise gets a
    /// warning in `messages` (its comparisons can differ).
    #[serde(default)]
    pub quick: bool,
    /// Count what would be cleaned (one pass, as `-checkforitm` and
    /// `-checkfordr` count), but change and save nothing.
    #[serde(default)]
    pub dry_run: bool,
    /// Where the quick mode saves the plugin; the path it was loaded from
    /// when omitted.
    pub output: Option<String>,
    /// Move an existing file at the output path to `<AppName>Edit Backups`
    /// before the quick mode replaces it. Defaults to true, as upstream.
    #[serde(default = "default_true")]
    pub backup: bool,
}

fn default_true() -> bool {
    true
}

/// A record or group a cleaning step changed or counted.
#[derive(Serialize, JsonSchema)]
pub struct CleanedRecord {
    /// The name as xEdit's message log shows it.
    pub name: String,
    /// The record signature, or `GRUP` for a group.
    pub signature: String,
    /// The load order FormID of a record, as eight hexadecimal digits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form_id: Option<String>,
}

impl From<&CleanedElement> for CleanedRecord {
    fn from(element: &CleanedElement) -> Self {
        CleanedRecord {
            name: element.name.clone(),
            signature: element.signature.clone(),
            form_id: element.form_id.clone(),
        }
    }
}

/// The result of one cleaning step.
#[derive(Serialize, JsonSchema)]
pub struct CleanStep {
    /// Nodes of the navigation tree the step walked ("Processed Records").
    pub processed: u64,
    /// Records (and, for the ITM, empty groups) removed, or references
    /// undeleted; counted only for a dry run.
    pub count: u64,
    /// Deleted navigation meshes, which can not be undeleted (UDR).
    pub deleted_navmeshes: u64,
    /// Other deleted references that can not be undeleted (UDR).
    pub not_undeleted: u64,
    pub records: Vec<CleanedRecord>,
    /// Records skipped or that could not be removed.
    pub skipped: Vec<CleanedRecord>,
}

impl From<&CleanReport> for CleanStep {
    fn from(report: &CleanReport) -> Self {
        CleanStep {
            processed: report.processed,
            count: report.cleaned.len() as u64,
            deleted_navmeshes: report.deleted_navmeshes,
            not_undeleted: report.not_undeleted,
            records: report.cleaned.iter().map(CleanedRecord::from).collect(),
            skipped: report.skipped.iter().map(CleanedRecord::from).collect(),
        }
    }
}

/// The filter for cleaning of a pass.
#[derive(Serialize, JsonSchema)]
pub struct CleanFilter {
    /// Nodes of the first pass of the filter.
    pub pass1: u64,
    /// Nodes of the second pass.
    pub pass2: u64,
    /// Nodes left.
    pub unfiltered: u64,
}

impl From<&FilterReport> for CleanFilter {
    fn from(report: &FilterReport) -> Self {
        CleanFilter {
            pass1: report.pass1,
            pass2: report.pass2,
            unfiltered: report.unfiltered,
        }
    }
}

/// One pass: the filter for cleaning, the UDR and the ITM removal.
#[derive(Serialize, JsonSchema)]
pub struct CleanPass {
    pub filter: CleanFilter,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub udr: Option<CleanStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub itm: Option<CleanStep>,
    /// The save after the pass (quick mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved: Option<FilesSaveResponse>,
}

#[derive(Serialize, JsonSchema)]
pub struct CleanResponse {
    /// The cleaned plugin.
    pub file: String,
    /// Nothing was changed: the counts are what a clean would do.
    pub dry_run: bool,
    pub passes: Vec<CleanPass>,
    /// Records and empty groups removed in all passes (the ITM count of
    /// xEdit's dirty information).
    pub itm: u64,
    /// References undeleted in all passes (the UDR count).
    pub udr: u64,
    /// Deleted navigation meshes of the plugin (the NAV count).
    pub deleted_navmeshes: u64,
    /// The plugin has changes in memory that are not saved.
    pub unsaved: bool,
    /// Whether the PNAM fill (`wbFillPNAM`) was on.
    pub fill_pnam: bool,
    /// Whether the plugins were loaded with the full record definitions
    /// (`wbSimpleRecords` off), as the quick clean modes load them.
    pub full_definitions: bool,
    /// The lines xEdit writes to its message log, without the times.
    pub messages: Vec<String>,
}

/// Core of `files.clean` for one plugin, which the tool modes of step 9
/// (`-quickautoclean`, `-quickclean`, `-checkforitm`, `-checkfordr`) call:
/// the passes as the request asks for them.
pub fn clean_file(
    target: &Arc<FileImpl>,
    files: &[Arc<FileImpl>],
    request: &CleanRequest,
) -> Result<CleanResponse, CommandError> {
    if !(request.itm || request.udr || request.quick) {
        return Err(CommandError::new("invalid_params", "pass itm, udr or quick"));
    }
    let settings = UdrSettings::for_game();
    let mut response = CleanResponse {
        file: target.get_name(),
        dry_run: request.dry_run,
        passes: Vec::new(),
        itm: 0,
        udr: 0,
        deleted_navmeshes: 0,
        unsaved: false,
        fill_pnam: fill_pnam() && can_sort_info(),
        full_definitions: !simple_records(),
        messages: Vec::new(),
    };
    if request.quick && (simple_records() || (can_sort_info() && !fill_pnam())) {
        response.messages.push(
            "<Warning: the plugins were not loaded as the quick clean mode loads them (full record definitions, PNAM fill); the result can differ from xEdit's>"
                .to_owned(),
        );
    }
    let passes = if request.quick && !request.dry_run { 3 } else { 1 };
    for pass_index in 0..passes {
        let pass = if request.quick {
            cleaning::quick_clean_pass(target, files, request.dry_run, &settings).map_err(edit_failed)?
        } else {
            single_pass(target, files, request, &settings)?
        };
        response.messages.extend(pass.filter.messages.iter().cloned());
        response.messages.push(format!(
            "Done: Applying Filter, [Pass 1] Processed Records: {}, [Pass 2] Processed Records: {}, Remaining unfiltered nodes: {}",
            pass.filter.pass1, pass.filter.pass2, pass.filter.unfiltered
        ));
        let do_udr = request.quick || request.udr;
        let do_itm = request.quick || request.itm;
        if do_udr {
            response.messages.extend(pass.udr.messages.iter().cloned());
            response.udr += pass.udr.cleaned.len() as u64;
            response.deleted_navmeshes = pass.udr.deleted_navmeshes;
        }
        if do_itm {
            response.messages.extend(pass.itm.messages.iter().cloned());
            response.itm += pass.itm.cleaned.len() as u64;
        }
        let was_unsaved = target.element_base().has_state(ElementState::esUnsaved);
        let mut report = CleanPass {
            filter: CleanFilter::from(&pass.filter),
            udr: do_udr.then(|| CleanStep::from(&pass.udr)),
            itm: do_itm.then(|| CleanStep::from(&pass.itm)),
            saved: None,
        };
        // `SaveChanged(True)` after the first two passes; the third pass is
        // not saved (UPSTREAM-QUIRK).
        let save = request.quick && !request.dry_run && pass_index < 2;
        if save && was_unsaved {
            let saved = save_file(target, request.output.clone(), false, request.backup)?;
            response.messages.push(format!("Saving: {}", target.get_name()));
            report.saved = Some(saved);
        }
        response.passes.push(report);
        if !was_unsaved {
            break;
        }
    }
    response.unsaved = target.element_base().has_state(ElementState::esUnsaved);
    Ok(response)
}

/// The filter for cleaning, then the UDR and the ITM removal as asked.
fn single_pass(
    target: &Arc<FileImpl>,
    files: &[Arc<FileImpl>],
    request: &CleanRequest,
    settings: &UdrSettings,
) -> Result<QuickCleanPass, CommandError> {
    let mut tree = NavTree::new(std::slice::from_ref(target));
    let filter = apply_filter(&mut tree, &FilterOptions::for_cleaning(), files).map_err(edit_failed)?;
    let start = tree
        .file_node(target)
        .ok_or_else(|| CommandError::new("internal", "the plugin has no node"))?;
    // The GUI selects the file node (`JumpTo` its header expands it, and
    // the tree sorts the children of an expanded node, `toAutoSort`).
    tree.sort_children(start);
    let mut pass = QuickCleanPass {
        filter,
        ..Default::default()
    };
    if request.udr {
        pass.udr = cleaning::undelete_and_disable_references(&mut tree, start, request.dry_run, settings)
            .map_err(edit_failed)?;
    }
    if request.itm {
        pass.itm = cleaning::remove_identical_to_master(&mut tree, start, request.dry_run).map_err(edit_failed)?;
    }
    Ok(pass)
}

fn edit_failed(message: String) -> CommandError {
    CommandError::new("edit_failed", message)
}

/// `files.clean` for the quick auto clean mode on `file`, the entry point
/// of `-quickautoclean`: the passes with their saves to `output` (the
/// loaded path when `None`).
pub fn quick_auto_clean(
    session: &mut Session,
    file: Option<&str>,
    output: Option<String>,
) -> Result<CleanResponse, CommandError> {
    let request = CleanRequest {
        file: file.map(str::to_owned),
        itm: false,
        udr: false,
        quick: true,
        dry_run: false,
        output,
        backup: true,
    };
    files_clean(session, request)
}

pub(crate) fn files_clean(session: &mut Session, request: CleanRequest) -> Result<CleanResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("files.clean")?;
    let target = session.file(request.file.as_deref())?;
    if target
        .get_file_states()
        .contains(xedit_core::interface::FileState::fsIsHardcoded)
    {
        return Err(CommandError::new(
            "not_editable",
            "the hardcoded records are not a plugin",
        ));
    }
    let files = crate::conflicts::session_files(session)?;
    // The checks of the GUI (`IsEditable`, `IsRemovable`) read
    // `wbEditAllowed`; a dry run counts as the edit would.
    let allowed = edit_allowed();
    if request.dry_run {
        set_edit_allowed(true);
    }
    // The GUI names the records in its message log with the shorter names.
    let shorter_names = display_shorter_names();
    set_display_shorter_names(true);
    // The records build deeply through the definitions: a thread with a
    // large stack, as the dump and the save use.
    let result = run_on_large_stack(|| clean_file(&target, &files, &request)).and_then(|result| result);
    set_display_shorter_names(shorter_names);
    set_edit_allowed(allowed);
    result
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CleanupInjectedRequest {
    /// The plugin whose records are cleaned up; the only loaded plugin when
    /// omitted.
    pub file: Option<String>,
    /// Load order FormIDs of the records of the plugin to clean up, as
    /// hexadecimal digits; every record of the plugin that refers to an
    /// injected record of a plugin that is not its master when empty (the
    /// GUI's "References injected" filter).
    #[serde(default)]
    pub form_ids: Vec<String>,
    /// Report the records and the masters, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct CleanupInjectedResponse {
    /// The plugin that holds the injected records, which the records were
    /// (or would be) copied into; none when no record qualified.
    pub reference_file: Option<String>,
    /// Masters added to that plugin for the copies.
    pub added_masters: Vec<String>,
    /// Records copied into it, whose references to injected records were
    /// removed in their own plugin.
    pub copied: Vec<CleanedRecord>,
    /// Records left alone: they refer to the injected records of no plugin,
    /// of several, or of another plugin than the first record.
    pub skipped: Vec<CleanedRecord>,
    pub dry_run: bool,
    /// The plugins that hold changes to save.
    pub changed_files: Vec<String>,
    pub messages: Vec<String>,
}

fn records_cleanup_injected(
    session: &mut Session,
    request: CleanupInjectedRequest,
) -> Result<CleanupInjectedResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("records.cleanup_injected")?;
    let file = session.file(request.file.as_deref())?;
    // `ReferencesInjected` reads the reference index, which the GUI builds
    // when it loads the plugins.
    session.ensure_refs()?;
    let records = if request.form_ids.is_empty() {
        let all = file.records();
        run_on_large_stack(move || {
            all.into_iter()
                .filter(|record| {
                    let injected = record.references_injected();
                    record.reset();
                    injected
                })
                .collect::<Vec<_>>()
        })?
    } else {
        let mut records = Vec::new();
        for form_id in &request.form_ids {
            let parsed = crate::commands::parse_form_id(form_id)?;
            let record = file.contained_record_by_load_order_form_id(parsed).ok_or_else(|| {
                CommandError::new(
                    "unknown_record",
                    format!("{} has no record {}", file.get_name(), parsed.to_string(false)),
                )
            })?;
            records.push(record);
        }
        records
    };
    let allowed = edit_allowed();
    if request.dry_run {
        set_edit_allowed(true);
    }
    let shorter_names = display_shorter_names();
    set_display_shorter_names(true);
    let dry_run = request.dry_run;
    let result = run_on_large_stack(move || cleaning::cleanup_injected(&records, dry_run));
    set_display_shorter_names(shorter_names);
    set_edit_allowed(allowed);
    let result = result?.map_err(edit_failed)?;
    let mut changed_files = Vec::new();
    if !request.dry_run && !result.copied.is_empty() {
        changed_files.push(file.get_name());
        if let Some(reference) = &result.reference_file {
            changed_files.push(reference.clone());
        }
    }
    Ok(CleanupInjectedResponse {
        reference_file: result.reference_file,
        added_masters: result.added_masters,
        copied: result.copied.iter().map(CleanedRecord::from).collect(),
        skipped: result.skipped.iter().map(CleanedRecord::from).collect(),
        dry_run: request.dry_run,
        changed_files,
        messages: result.messages,
    })
}

/// Runs `work` on a thread with a large stack: the records build deeply
/// through the definitions.
pub(crate) fn run_on_large_stack<T: Send>(work: impl FnOnce() -> T + Send) -> Result<T, CommandError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(xedit_core::threads::STACK_SIZE)
            .spawn_scoped(scope, work)
            .map_err(|error| CommandError::new("internal", error.to_string()))?
            .join()
            .map_err(|_| CommandError::new("internal", "the worker thread panicked"))
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "records.cleanup_injected",
        "Copy the records that refer to injected records of a plugin which is not their master into that plugin, and remove those references from the originals (mniNavCleanupInjectedClick).",
        true,
        records_cleanup_injected,
    );
    registry.register(
        "files.clean",
        "Clean a plugin: remove the records identical to their master (mniNavRemoveIdenticalToMasterClick), undelete and disable the deleted references (mniNavUndeleteAndDisableReferencesClick), or run the quick auto clean mode (-quickautoclean), which saves.",
        true,
        files_clean,
    );
}
