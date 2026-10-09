// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas
// (mniNavRemoveIdenticalToMasterClick,
// mniNavUndeleteAndDisableReferencesClick, mniNavCleanupInjectedClick,
// mniNavCleaningObsoleteClick, the quick clean steps of
// tmrGeneratorTimer), Core/wbImplementation.pas
// (GetInjectionSourceFiles, GetReferencesInjected, RemoveInjected)

//! The cleaning functions of the main form: "Remove Identical to Master
//! records" (ITM), "Undelete and Disable References" (UDR) and "Cleanup
//! injected records", over a [`NavTree`] that the filter for cleaning
//! ([`FilterOptions::for_cleaning`]) was applied to, and one pass of the
//! quick clean mode (`-quickclean`, `-quickautoclean`), which runs the
//! three steps on the one plugin of the mode ([`quick_clean_pass`]).
//!
//! Both walks go from the last node of the selection to the node after it,
//! as the GUI walks them, so the children of a node come before it: a group
//! whose records all went is empty by the time the walk reaches it and goes
//! too, and a cell whose children all went is a node without children and
//! is judged by its own (inherited) status. The count of removed records
//! includes those groups, as upstream counts them.
//!
//! Not ported: `wbAllowMakePartial` (the partial forms that `-AllowMakePartial`
//! makes of a cell or worldspace with children; `MakePartialForm` is owed
//! from phase 3), so a record with children is never removed or made
//! partial. The dirty information for LOOT (`LOOTPluginInfos`) is the
//! counts of the reports.

use std::sync::Arc;

use xedit_core::implementation::{ElementImpl, FileImpl, MainRecordImpl};
use xedit_core::interface::globals::{GameMode, game_mode, is_fallout3, translation_mode};
use xedit_core::interface::misc::Variant;
use xedit_core::interface::types::{ConflictThis, Signature};
use xedit_core::interface::{Container, Element, ElementRef, MainRecord};

use crate::filter::{FilterOptions, FilterReport, NavTree, NodeId, apply_filter};

/// The message of upstream when the tree was not filtered for cleaning.
pub const FILTER_REQUIRED: &str = "To use this function you need to apply a filter with *only* the option \"Conflict status inherited by parent\" active and ModGroups, \"Show Master and Leafs\", and \"Quick Show Conflict\" mode disabled.";

/// Port of `mniNavCleaningObsoleteClick`: the message the obsolete manual
/// cleaning menu items show.
pub const CLEANING_OBSOLETE: &str = "This function has been made obsolete by the introduction of Quick Auto Clean mode.\r\n\r\nIf you have used this function because you were following a guide, please be aware that the guide is outdated and no longer applies.\r\n\r\nFor more information about Quick Auto Clean mode, please check the What's New document or the online help (press the help button in the top right corner of the main form).\r\n\r\nYou can hide this function by checking the \"Hide Manual Cleaning functions\" Option.";

/// The settings of "Undelete and Disable References" (`wbUDRSetXESP` and
/// the rest, the "Cleaning" tab of the options).
#[derive(Debug, Clone, PartialEq)]
pub struct UdrSettings {
    /// `wbUDRSetXESP`: the reference gets the player as its enable parent,
    /// opposite of the parent.
    pub set_xesp: bool,
    /// `wbUDRSetScale` with `wbUDRSetScaleValue`.
    pub set_scale: bool,
    pub scale_value: f64,
    /// `wbUDRSetZ` with `wbUDRSetZValue`: a reference that is not
    /// persistent moves below the world.
    pub set_z: bool,
    pub z_value: f64,
    /// `wbUDRSetMSTT` with `wbUDRSetMSTTValue`: in Fallout 3 a reference to
    /// a moveable static gets this base record (`AshPile01`).
    pub set_mstt: bool,
    pub mstt_value: u32,
}

impl UdrSettings {
    /// The defaults of `wbInterface.pas` with the game's changes of
    /// `xeInit.pas` (Fallout 3 and New Vegas do not move the reference,
    /// and would move it to -15000).
    pub fn for_game() -> Self {
        let fallout = matches!(game_mode(), GameMode::gmFO3 | GameMode::gmFNV);
        UdrSettings {
            set_xesp: true,
            set_scale: false,
            scale_value: 0.0,
            set_z: !fallout,
            z_value: if fallout { -15000.0 } else { -30000.0 },
            set_mstt: true,
            mstt_value: 0x0000_001B,
        }
    }
}

/// What a cleaning function did to one record or group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanedElement {
    /// The name as the message log of the GUI shows it.
    pub name: String,
    /// The signature of a main record; `GRUP` for a group.
    pub signature: String,
    /// The load order FormID of a main record.
    pub form_id: Option<String>,
}

/// The result of [`remove_identical_to_master`] or
/// [`undelete_and_disable_references`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanReport {
    /// Nodes walked ("Processed Records").
    pub processed: u64,
    /// The records and groups removed, or the references undeleted (or
    /// counted, for a count only).
    pub cleaned: Vec<CleanedElement>,
    /// The deleted navigation meshes the UDR can not undelete.
    pub deleted_navmeshes: u64,
    /// Other deleted references the UDR can not undelete (injected, without
    /// a base record, a tree with LOD in New Vegas).
    pub not_undeleted: u64,
    /// The elements that were skipped (`Skipping:`) or could not be removed
    /// (`Can't remove:`).
    pub skipped: Vec<CleanedElement>,
    /// The lines the GUI adds to its message log, without the times.
    pub messages: Vec<String>,
}

fn cleaned_element(element: &ElementRef) -> CleanedElement {
    let record = element.as_element_impl().and_then(ElementImpl::main_record_impl);
    CleanedElement {
        name: element.get_name(),
        signature: record
            .as_ref()
            .map_or_else(|| "GRUP".to_owned(), |record| record.get_signature().to_string()),
        form_id: record.map(|record| record.get_load_order_form_id().to_string(false)),
    }
}

fn record_of(element: &ElementRef) -> Option<Arc<MainRecordImpl>> {
    element.as_element_impl().and_then(ElementImpl::main_record_impl)
}

/// The walk of both functions: from the last node below `start` back to the
/// node after `start` (which is not visited).
fn walk(tree: &mut NavTree, start: NodeId, mut visit: impl FnMut(&mut NavTree, NodeId)) -> u64 {
    let mut count = 0;
    let mut current = tree.get_last(Some(start));
    // A node without children is its own last node, and nothing is walked.
    if current == Some(start) {
        return 0;
    }
    while let Some(id) = current {
        let next = tree.get_previous(id);
        visit(tree, id);
        count += 1;
        current = next.filter(|&next| next != start);
    }
    count
}

fn check_filter(tree: &NavTree) -> Result<(), String> {
    match tree.applied_filter() {
        Some(options) if options.is_cleaning_filter() => Ok(()),
        _ => Err(FILTER_REQUIRED.to_owned()),
    }
}

/// Port of `mniNavRemoveIdenticalToMasterClick` on the selected node
/// `start` (the file node in the quick clean mode). `count_only` is the
/// `tmCheckForITM` mode: the records are counted, not removed.
pub fn remove_identical_to_master(tree: &mut NavTree, start: NodeId, count_only: bool) -> Result<CleanReport, String> {
    if translation_mode() {
        return Ok(CleanReport::default());
    }
    check_filter(tree)?;
    let operation = if count_only { "Count" } else { "Remov" };
    let mut report = CleanReport::default();
    report.processed = walk(tree, start, |tree, id| {
        let Some(element) = tree.data(id).element.clone() else {
            return;
        };
        let record = record_of(&element);
        let data = tree.data(id);
        let is_group =
            record.is_none() && element.get_element_type() == xedit_core::interface::types::ElementType::etGroupRecord;
        let identical = data.conflict_this == ConflictThis::ctIdenticalToMaster
            || (data.conflict_this == ConflictThis::ctConflictBenign
                && record
                    .as_ref()
                    .is_some_and(|record| record.get_signature() == Signature::new(b"NAVM")))
            || is_group;
        // UPSTREAM-QUIRK: without `wbAllowMakePartial` only a node without
        // children is removed; a group counts whatever its status.
        if tree.child_count(id) != 0 || !identical {
            return;
        }
        if record
            .as_ref()
            .is_some_and(|record| master_or_self(record).is_injected())
        {
            return;
        }
        if !element.get_is_removable() {
            report.messages.push(format!("Can't remove: {}", element.get_name()));
            report.skipped.push(cleaned_element(&element));
            return;
        }
        let cleaned = cleaned_element(&element);
        report.messages.push(format!("{operation}ing: {}", cleaned.name));
        if !count_only {
            if let Some(container) = tree.data(id).container.clone()
                && container.get_element_id() != element.get_element_id()
            {
                container.remove();
            }
            element.remove();
            tree.delete_node(id);
        }
        report.cleaned.push(cleaned);
    });
    report.messages.push(format!(
        "[{operation}ing \"Identical to Master\" records done]  Processed Records: {}, {operation}ed Records: {}",
        report.processed,
        report.cleaned.len()
    ));
    Ok(report)
}

fn master_or_self(record: &Arc<MainRecordImpl>) -> Arc<MainRecordImpl> {
    record.master().unwrap_or_else(|| record.clone())
}

/// The placed records "Undelete and Disable References" undeletes.
const UNDELETED: [&[u8; 4]; 12] = [
    b"REFR", b"PGRE", b"PMIS", b"ACHR", b"ACRE", b"NAVM", b"PARW", b"PBAR", b"PBEA", b"PCON", b"PFLA", b"PHZD",
];

/// Port of `mniNavUndeleteAndDisableReferencesClick` on the selected node
/// `start`. `count_only` is the `tmCheckForDR` mode: the references are
/// counted, not changed.
pub fn undelete_and_disable_references(
    tree: &mut NavTree,
    start: NodeId,
    count_only: bool,
    settings: &UdrSettings,
) -> Result<CleanReport, String> {
    if translation_mode() {
        return Ok(CleanReport::default());
    }
    check_filter(tree)?;
    let operation = if count_only { "Count" } else { "Undelet" };
    let mut report = CleanReport::default();
    // Upstream keeps `Element` across the records, so the scale and the
    // MSTT steps can find the element of an earlier record.
    let mut element: Option<ElementRef> = None;
    report.processed = walk(tree, start, |tree, id| {
        let Some(record) = tree.data(id).element.as_ref().and_then(record_of) else {
            return;
        };
        let signature = record.get_signature();
        if !(record.get_is_editable()
            && record.get_is_deleted()
            && UNDELETED.iter().any(|expected| signature == Signature::new(expected)))
        {
            return;
        }
        let name = record.get_name();
        // `canUndelete`.
        let links_to = master_or_self(&record).get_base_record();
        let can_undelete = if signature == Signature::new(b"NAVM") {
            report.deleted_navmeshes += 1;
            false
        } else if record.is_injected()
            || links_to.is_none()
            // A tree with LOD in New Vegas.
            || (game_mode() == GameMode::gmFNV
                && links_to.as_ref().is_some_and(|base| {
                    base.get_signature() == Signature::new(b"TREE") && base.get_flags().0 & 0x40 != 0
                }))
        {
            report.not_undeleted += 1;
            false
        } else {
            true
        };
        if !can_undelete {
            report.messages.push(format!("Skipping: {name}"));
            let element: ElementRef = record.clone();
            report.skipped.push(cleaned_element(&element));
            return;
        }
        report.messages.push(format!("{operation}ing: {name}"));
        let self_element: ElementRef = record.clone();
        let cleaned = cleaned_element(&self_element);
        if !count_only {
            undelete_and_disable(&record, settings, &mut element, &mut report.messages);
        }
        report.cleaned.push(cleaned);
    });
    report.messages.push(format!(
        "[{operation}ing and Disabling References done]  Processed Records: {}, {operation}ed Records: {}",
        report.processed,
        report.cleaned.len()
    ));
    if report.deleted_navmeshes > 0 {
        report.messages.push(format!(
            "<Warning: Plugin contains {} deleted NavMeshes which can not be undeleted>",
            report.deleted_navmeshes
        ));
    }
    if report.not_undeleted > 0 {
        report.messages.push(format!(
            "<Warning: Plugin contains {} deleted references which can not be undeleted>",
            report.not_undeleted
        ));
    }
    Ok(report)
}

/// The edits of the UDR on one reference.
fn undelete_and_disable(
    record: &Arc<MainRecordImpl>,
    settings: &UdrSettings,
    element: &mut Option<ElementRef>,
    messages: &mut Vec<String>,
) {
    // `IsDeleted := True; IsDeleted := False;`: the first does nothing on a
    // deleted record.
    if let Err(error) = record.set_is_deleted(false) {
        messages.push(error);
        return;
    }
    if !record.get_is_persistent()
        && settings.set_z
        && let Some((x, y, _)) = record.get_position()
    {
        record.set_record_position((x, y, settings.z_value));
    }
    record.remove_element_by_name("Enable Parent");
    record.remove_element_by_name("XTEL");
    record.set_is_initially_disabled(true);
    if settings.set_xesp
        && let Ok(Some(xesp)) = record.add("XESP", true)
        && let Some(container) = xesp.as_container()
    {
        let _ = container.set_element_native_value("Reference", Variant::UInt(0x14));
        if let Some(flags) = container.get_element(1) {
            let _ = flags.set_native_value(Variant::UInt(1));
        }
    }
    if settings.set_scale {
        if record.get_record_by_signature(Signature::new(b"XSCL")).is_none() {
            *element = record.add("XSCL", true).ok().flatten();
        }
        // UPSTREAM-QUIRK: when the reference has a scale, the value goes to
        // the element the last step found (`Element` is not reset).
        if let Some(element) = element.as_ref() {
            let _ = element.set_native_value(Variant::Float(settings.scale_value));
        }
    }
    if settings.set_mstt && is_fallout3() {
        *element = record.get_record_by_signature(Signature::new(b"NAME"));
        if let Some(name) = element.as_ref()
            && name
                .get_links_to()
                .and_then(|linked| linked.into_main_record())
                .is_some_and(|linked| linked.get_signature() == Signature::new(b"MSTT"))
        {
            let _ = name.set_native_value(Variant::UInt(u64::from(settings.mstt_value)));
        }
    }
}

/// The report of one pass of the quick clean mode.
#[derive(Debug, Clone, Default)]
pub struct QuickCleanPass {
    pub filter: FilterReport,
    pub udr: CleanReport,
    pub itm: CleanReport,
}

/// One pass of the quick clean mode on `target` (`tmrGeneratorTimer` with
/// `xeQuickClean`): the filter for cleaning over the tree of the plugin
/// (`ReInitTree` shows only the plugin of the mode), then "Undelete and
/// Disable References" and "Remove Identical to Master records" on its file
/// node. With `count_only` nothing changes (the counts of `tmCheckForDR`
/// and `tmCheckForITM`).
pub fn quick_clean_pass(
    target: &Arc<FileImpl>,
    files: &[Arc<FileImpl>],
    count_only: bool,
    settings: &UdrSettings,
) -> Result<QuickCleanPass, String> {
    let mut tree = NavTree::new(std::slice::from_ref(target));
    let filter = apply_filter(&mut tree, &FilterOptions::for_cleaning(), files)?;
    let start = tree
        .file_node(target)
        .ok_or_else(|| format!("{} has no node", target.get_name()))?;
    // The GUI selects the file node (`JumpTo` its header expands it, and
    // the tree sorts the children of an expanded node, `toAutoSort`).
    tree.sort_children(start);
    let udr = undelete_and_disable_references(&mut tree, start, count_only, settings)?;
    let itm = remove_identical_to_master(&mut tree, start, count_only)?;
    Ok(QuickCleanPass { filter, udr, itm })
}

/// The result of [`cleanup_injected`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InjectedCleanup {
    /// The plugin that holds the injected records the selection refers to
    /// (`ReferenceFile`), which the records were copied into.
    pub reference_file: Option<String>,
    /// The masters added to the reference file.
    pub added_masters: Vec<String>,
    /// The records copied into the reference file.
    pub copied: Vec<CleanedElement>,
    /// Records that refer to injected records of no file or of more than
    /// one, or of another file than the first record of the selection.
    pub skipped: Vec<CleanedElement>,
    /// The lines the GUI adds to its message log.
    pub messages: Vec<String>,
}

/// Port of `mniNavCleanupInjectedClick` on the selected records: the
/// records that refer to the injected records of exactly one plugin (the
/// plugin of the first such record of the selection) are copied into that
/// plugin as overrides, with the masters the copies need, and their
/// references to injected records of plugins that are not masters of their
/// own file are removed. With `dry_run` only the selection and the masters
/// are reported.
pub fn cleanup_injected(records: &[Arc<MainRecordImpl>], dry_run: bool) -> Result<InjectedCleanup, String> {
    let mut result = InjectedCleanup::default();
    if translation_mode() {
        return Ok(result);
    }
    let mut reference_file: Option<Arc<FileImpl>> = None;
    let mut selected = Vec::new();
    for record in records {
        let element: ElementRef = record.clone();
        let sources = record.injection_source_files();
        let [source] = sources.as_slice() else {
            result.skipped.push(cleaned_element(&element));
            continue;
        };
        match &reference_file {
            None => reference_file = Some(source.clone()),
            Some(file) if !Arc::ptr_eq(file, source) => {
                result.skipped.push(cleaned_element(&element));
                continue;
            }
            Some(_) => {}
        }
        selected.push(record.clone());
    }
    let Some(reference_file) = reference_file else {
        return Ok(result);
    };
    result.reference_file = Some(reference_file.get_name());

    // The masters of the records and of their containers.
    let mut masters = xedit_core::implementation::copy::FilesSet::new();
    for record in &selected {
        record.report_required_masters(&mut masters, false, true, false);
        let mut container = record.get_container();
        while let Some(current) = container {
            if let Some(current) = current.as_element_impl() {
                current.report_required_masters(&mut masters, false, false, false);
            }
            container = current.get_container();
        }
    }
    let own: Vec<String> = reference_file
        .masters()
        .iter()
        .map(|master| master.get_name())
        .collect();
    result.added_masters = masters
        .files()
        .iter()
        .map(|file| file.get_name())
        .filter(|name| !name.eq_ignore_ascii_case(&reference_file.get_name()))
        .filter(|name| !own.iter().any(|own| own.eq_ignore_ascii_case(name)))
        .collect();
    result.added_masters.sort_by_key(|name| name.to_ascii_lowercase());
    result.added_masters.dedup();
    for record in &selected {
        let element: ElementRef = record.clone();
        result.copied.push(cleaned_element(&element));
    }
    if dry_run {
        return Ok(result);
    }
    xedit_core::implementation::copy::add_required_masters(&masters, &reference_file)?;
    let args = xedit_core::interface::CopyArgs {
        as_new: false,
        deep_copy: true,
        prefix_remove: String::new(),
        suffix_remove: String::new(),
        prefix: String::new(),
        suffix: String::new(),
        allow_overwrite: false,
    };
    for record in &selected {
        let element: ElementRef = record.clone();
        xedit_core::implementation::copy::copy_element_to_file(&element, &reference_file, &args)?;
        if record.remove_injected(false) {
            result.messages.push(format!(
                "Injected references in {} could not all be removed automatically.",
                record.get_name()
            ));
        }
    }
    Ok(result)
}
