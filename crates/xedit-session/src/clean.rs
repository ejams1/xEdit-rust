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
//! plain quick clean mode (`-quickclean`) leaves `xeQuickCleanAutoSave` off:
//! one pass, no save (`CleanRequest::quick_auto_save`). The mode loads the
//! plugins with its own settings (`xeInit.pas`: the full record
//! definitions, the PNAM fill), which the CLI applies for
//! `xedit clean --quick` (`commands::set_quick_clean_on_load`); see
//! [`quick_auto_clean`].

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_analysis::cleaning::{self, CleanReport, CleanedElement, QuickCleanPass, UdrSettings};
use xedit_analysis::filter::{FilterOptions, FilterReport, NavTree, apply_filter};
use xedit_core::implementation::{ElementImpl, ElementState, FileImpl};
use xedit_core::interface::globals;
use xedit_core::interface::globals::{
    GameMode, can_sort_info, display_shorter_names, edit_allowed, fill_pnam, set_display_shorter_names,
    set_edit_allowed, simple_records,
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
    /// The plugins to clean (the navigation tree's selection): each one is
    /// cleaned in the order given, as one click of the GUI's cleaning menu
    /// items over a selection of several file nodes does. `file` is used
    /// when empty. The quick clean mode cleans exactly one plugin.
    #[serde(default)]
    pub files: Vec<String>,
    /// Remove the records that are identical to their master, and the
    /// groups left empty ("Remove Identical to Master records").
    #[serde(default)]
    pub itm: bool,
    /// Undelete the deleted references and disable them ("Undelete and
    /// Disable References").
    #[serde(default)]
    pub udr: bool,
    /// The quick clean mode (`-quickclean`, `-quickautoclean`): UDR and ITM
    /// in one pass. xEdit loads the plugins for it with the full record
    /// definitions and the PNAM fill; `xedit clean --quick` does too, and a
    /// session loaded otherwise gets a warning in `messages` (its
    /// comparisons can differ).
    #[serde(default)]
    pub quick: bool,
    /// Whether the quick mode saves, as `-quickautoclean` (`-qac`) does
    /// through `xeQuickCleanAutoSave`: the save after the pass and again
    /// while a pass changes the plugin. `-quickclean` (`-qc`) leaves the
    /// flag off and cleans in memory only. Defaults to true, as the quick
    /// mode of the CLI (`xedit clean --quick`) is the quick auto clean mode.
    #[serde(default = "default_true")]
    pub quick_auto_save: bool,
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
    /// `wbAllowMakePartial` (`-IKnowWhatImDoing -AllowMakePartial`): "Remove
    /// Identical to Master records" does not remove a record identical to
    /// its master that has children (a cell or worldspace), it makes it a
    /// partial form (`MakePartialForm`) and counts it as removed, and it
    /// removes a partial form whose child group is empty. Off by default,
    /// as the release without the switch.
    #[serde(default)]
    pub allow_make_partial: bool,
    /// The "Cleaning" tab of xEdit's options (`[Options] UDRSetXESP` of the
    /// settings file), for "Undelete and Disable References": the
    /// undeleted references get the player as an opposite enable parent
    /// (`wbUDRSetXESP`). The game's default (`true`) when omitted.
    pub udr_xesp: Option<bool>,
    /// `wbUDRSetScale`: the undeleted references get the scale
    /// `udr_scale_value`. Default `false`.
    pub udr_scale: Option<bool>,
    /// `wbUDRSetScaleValue`, a single precision value. Default `0.0`.
    pub udr_scale_value: Option<f64>,
    /// `wbUDRSetZ`: an undeleted reference that is not persistent moves to
    /// the height `udr_z_value`. Default `true`, `false` for Fallout 3 and
    /// New Vegas (`xeInit.pas`).
    pub udr_z: Option<bool>,
    /// `wbUDRSetZValue`, a single precision value. Default `-30000`,
    /// `-15000` for Fallout 3 and New Vegas.
    pub udr_z_value: Option<f64>,
    /// `wbUDRSetMSTT`: in Fallout 3 an undeleted reference to a moveable
    /// static gets the base record `udr_mstt_value`. Default `true`.
    pub udr_mstt: Option<bool>,
    /// `wbUDRSetMSTTValue` as the options dialog takes it: hexadecimal
    /// digits (`0000001B`, `AshPile01`, by default); text that is not a
    /// number keeps the default (`StrToInt64Def`).
    pub udr_mstt_value: Option<String>,
    /// The file node was reached with `JumpTo` (its header), which expands
    /// it, so the tree sorts its children (`toAutoSort`): the
    /// `-checkforitm` and `-checkfordr` modes. A selection clicked in the
    /// tree leaves the file nodes collapsed, their children in the order of
    /// the file.
    #[serde(skip)]
    #[schemars(skip)]
    pub jump_to: bool,
}

fn default_true() -> bool {
    true
}

impl Default for CleanRequest {
    /// Nothing asked for, the backup on, every setting the game's: the
    /// fields a caller sets come on top (`..Default::default()`).
    fn default() -> Self {
        CleanRequest {
            file: None,
            files: Vec::new(),
            itm: false,
            udr: false,
            quick: false,
            quick_auto_save: true,
            dry_run: false,
            output: None,
            backup: true,
            allow_make_partial: false,
            udr_xesp: None,
            udr_scale: None,
            udr_scale_value: None,
            udr_z: None,
            udr_z_value: None,
            udr_mstt: None,
            udr_mstt_value: None,
            jump_to: false,
        }
    }
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

/// One entry of the dirty-information list (`TLOOTPluginInfo` of
/// `xeMainForm.pas`): the counts the cleaning handlers stored for one plugin
/// and CRC32. xEdit's "LOOT Masterlist Entries" report
/// (`mniNavLOManagersDirtyInfoClick`) prints them. Each handler sets only
/// its own counts and leaves the others as they were.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DirtyInfo {
    /// `Plugin`: the name of the file the last record the walk visited
    /// belongs to, as `TwbFile.FileName` gives it.
    pub plugin: String,
    /// `CRC32`: the file's CRC32 as the walk saw it (of the loaded bytes, or
    /// of the last save when the walk ran after one).
    pub crc32: u32,
    /// `ITM`: the records (and groups) "Remove Identical to Master records"
    /// removed, partial forms included.
    pub itm: u64,
    /// `UDR`: the references "Undelete and Disable References" undeleted.
    pub udr: u64,
    /// `NAV`: the deleted navigation meshes the UDR could not undelete.
    pub nav: u64,
}

impl DirtyInfo {
    /// `LOOTDirtyInfo` of `xeMainForm.pas`: the LOOT masterlist entry of one
    /// plugin, without the trailing newline. `file_changed` is
    /// `(i = 0) or not SameText(plugin, previous plugin)`: the entry starts
    /// with its name when it is the first of its plugin.
    pub fn loot_entry(&self, file_changed: bool) -> String {
        let app = globals::app_name();
        let (major, minor, release, build) = super::script_adapter::WB_VERSION;
        let version = format!("{major}.{minor}.{release}{build}");
        let util = |url: bool| {
            if url {
                format!("[{}Edit v{}]({})", app, version, globals::nexus_mods_url())
            } else {
                format!("{}Edit v{}", app, version)
            }
        };
        let mut result = String::new();
        if self.itm != 0 || self.udr != 0 || self.nav != 0 {
            if file_changed {
                result.push_str(&format!(
                    "\r\n  - name: '{}'\r\n    dirty:\r\n",
                    self.plugin.replace('\'', "''")
                ));
            }
            // No line break before the first line of a further entry of
            // the same plugin: the message starts with it.
            if self.nav != 0 {
                result.push_str("      - <<: *reqManualFix");
            } else {
                result.push_str("      - <<: *quickClean");
            }
            result.push_str(&format!("\r\n        crc: 0x{:08X}", self.crc32));
            result.push_str(&format!("\r\n        util: '{}'", util(true)));
            if self.itm != 0 {
                result.push_str(&format!("\r\n        itm: {}", self.itm));
            }
            if self.udr != 0 {
                result.push_str(&format!("\r\n        udr: {}", self.udr));
            }
            if self.nav != 0 {
                result.push_str(&format!("\r\n        nav: {}", self.nav));
            }
        } else {
            if file_changed {
                result.push_str(&format!("\r\n  - name: '{}'\r\n", self.plugin.replace('\'', "''")));
            }
            result.push_str("    clean:");
            result.push_str(&format!("\r\n      - crc: 0x{:08X}", self.crc32));
            result.push_str(&format!("\r\n        util: '{}'", util(false)));
        }
        result
    }

    /// `BOSSDirtyInfo` of `xeMainForm.pas`: the BOSS entry of one plugin
    /// (only the report of Oblivion prints them).
    pub fn boss_entry(&self) -> String {
        if self.itm == 0 && self.udr == 0 {
            return String::new();
        }
        format!(
            "\r\n{}\r\n  IF CHECKSUM(\"{}\", {:08X}) DIRTY: {} ITM, {} UDR records. Needs {}Edit cleaning: \"http://cs.elderscrolls.com/index.php?title=TES4Edit_Cleaning_Guide\"",
            self.plugin,
            self.plugin,
            self.crc32,
            self.itm,
            self.udr,
            globals::app_name()
        )
    }
}

/// Port of `mniNavLOManagersDirtyInfoClick`: the lines the message log gets
/// for the dirty-information list, in order. The entries of one plugin come
/// after each other without a name line.
pub fn dirty_info_lines(infos: &[DirtyInfo]) -> Vec<String> {
    let mut lines = Vec::new();
    if infos.is_empty() {
        return lines;
    }
    lines.push(String::new());
    lines.push("LOOT Masterlist Entries".to_owned());
    let mut boss = false;
    for (index, info) in infos.iter().enumerate() {
        let file_changed = index == 0 || !info.plugin.eq_ignore_ascii_case(&infos[index - 1].plugin);
        for part in info.loot_entry(file_changed).split("\r\n") {
            lines.push(part.to_owned());
        }
        if info.itm != 0 || info.udr != 0 {
            boss = globals::game_mode() == GameMode::gmTES4;
        }
    }
    lines.push(String::new());
    if boss {
        lines.push("BOSS Masterlist Entries".to_owned());
        for info in infos {
            for part in info.boss_entry().split("\r\n") {
                lines.push(part.to_owned());
            }
        }
    }
    lines
}

#[derive(Serialize, JsonSchema)]
pub struct CleanResponse {
    /// The cleaned plugin.
    pub file: String,
    /// Every plugin that was cleaned (the request's selection), in the
    /// order of the navigation tree.
    pub files: Vec<String>,
    /// The dirty information the steps stored (`LOOTPluginInfos`, which the
    /// session keeps across calls: `files.dirty_info` prints all of it).
    pub dirty_info: Vec<DirtyInfo>,
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

/// The UDR settings of a request: the game's defaults (the "Cleaning" tab of
/// the options dialog as `wbInterface.pas` initialises it and `xeInit.pas`
/// changes it, [`UdrSettings::for_game`]), with the settings the request
/// names overriding them.
pub fn udr_settings(request: &CleanRequest) -> UdrSettings {
    let mut settings = UdrSettings::for_game();
    if let Some(value) = request.udr_xesp {
        settings.set_xesp = value;
    }
    if let Some(value) = request.udr_scale {
        settings.set_scale = value;
    }
    // `wbUDRSetScaleValue` and `wbUDRSetZValue` are `Single`.
    if let Some(value) = request.udr_scale_value {
        settings.scale_value = f64::from(value as f32);
    }
    if let Some(value) = request.udr_z {
        settings.set_z = value;
    }
    if let Some(value) = request.udr_z_value {
        settings.z_value = f64::from(value as f32);
    }
    if let Some(value) = request.udr_mstt {
        settings.set_mstt = value;
    }
    // `StrToInt64Def('$' + edUDRSetMSTTValue.Text, wbUDRSetMSTTValue)`;
    // the value is an `Int64` the reference's `NAME` takes as a FormID.
    if let Some(text) = &request.udr_mstt_value
        && let Some(value) = parse_hex_def(text)
    {
        settings.mstt_value = value as u32;
    }
    settings
}

/// `StrToInt64Def('$' + text, ...)`: hexadecimal digits that fit an
/// `Int64`, else `None` (the default stays).
fn parse_hex_def(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(text, 16).ok().map(|value| value as i64)
}

/// Core of `files.clean` for one plugin, which the tool modes of step 9
/// (`-quickautoclean`, `-quickclean`, `-checkforitm`, `-checkfordr`) call:
/// the passes as the request asks for them.
pub fn clean_file(
    target: &Arc<FileImpl>,
    files: &[Arc<FileImpl>],
    request: &CleanRequest,
) -> Result<CleanResponse, CommandError> {
    clean_targets(std::slice::from_ref(target), files, request, &mut Vec::new())
}

/// Core of `files.clean` for the request's selection: the passes as the
/// request asks for them, over `targets` (the file nodes the caller
/// selected) with `loaded` the files of the session (the tree the GUI's
/// filter walks). `dirty_info` is the session's `LOOTPluginInfos`, which
/// the steps update.
pub fn clean_targets(
    targets: &[Arc<FileImpl>],
    loaded: &[Arc<FileImpl>],
    request: &CleanRequest,
    dirty_info: &mut Vec<DirtyInfo>,
) -> Result<CleanResponse, CommandError> {
    if !(request.itm || request.udr || request.quick) {
        return Err(CommandError::new("invalid_params", "pass itm, udr or quick"));
    }
    // `GetSortedSelection(True)`: the file nodes in the order of the tree,
    // which is the load order.
    let mut targets = targets.to_vec();
    targets.sort_by_key(|target| {
        loaded
            .iter()
            .position(|file| Arc::ptr_eq(file, target))
            .unwrap_or(usize::MAX)
    });
    targets.dedup_by(|a, b| Arc::ptr_eq(a, b));
    let targets = targets.as_slice();
    let Some(first) = targets.first() else {
        return Err(CommandError::new("no_file", "no plugin to clean"));
    };
    let allow_make_partial = request.allow_make_partial || globals::allow_make_partial();
    if request.quick && targets.len() > 1 {
        // `Exactly one module must be selected for Quick Clean mode.`
        return Err(CommandError::new(
            "invalid_params",
            "the quick clean mode cleans exactly one plugin",
        ));
    }
    let settings = udr_settings(request);
    let mut response = CleanResponse {
        file: first.get_name(),
        files: targets.iter().map(|target| target.get_name()).collect(),
        dry_run: request.dry_run,
        passes: Vec::new(),
        itm: 0,
        udr: 0,
        deleted_navmeshes: 0,
        unsaved: false,
        fill_pnam: fill_pnam() && can_sort_info(),
        full_definitions: !simple_records(),
        dirty_info: Vec::new(),
        messages: Vec::new(),
    };
    if request.quick && (simple_records() || (can_sort_info() && !fill_pnam())) {
        response.messages.push(
            "<Warning: the plugins were not loaded as the quick clean mode loads them (full record definitions, PNAM fill); the result can differ from xEdit's: load with `session.load --mode quickautoclean` (or the legacy -quickautoclean) to run the mode as xEdit does>"
                .to_owned(),
        );
    }
    // `xeQuickCleanAutoSave` is set by `-quickautoclean` only: the release
    // runs one pass and no save for `-quickclean`, up to three passes with a
    // save after the first two otherwise (`if WasUnsaved then [next pass]`
    // is inside `if xeQuickCleanAutoSave`).
    let passes = if request.quick && request.quick_auto_save && !request.dry_run {
        3
    } else {
        1
    };
    for pass_index in 0..passes {
        let pass = if request.quick {
            cleaning::quick_clean_pass(first, loaded, request.dry_run, &settings, allow_make_partial)
                .map_err(edit_failed)?
        } else {
            selection_pass(targets, loaded, request, &settings, allow_make_partial)?
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
            store_dirty_info(dirty_info, &pass.udr, None);
        }
        if do_itm {
            response.messages.extend(pass.itm.messages.iter().cloned());
            response.itm += pass.itm.cleaned.len() as u64;
            store_dirty_info(dirty_info, &pass.itm, Some(pass.itm.cleaned.len() as u64));
        }
        let was_unsaved = targets
            .iter()
            .any(|target| target.element_base().has_state(ElementState::esUnsaved));
        let mut report = CleanPass {
            filter: CleanFilter::from(&pass.filter),
            udr: do_udr.then(|| CleanStep::from(&pass.udr)),
            itm: do_itm.then(|| CleanStep::from(&pass.itm)),
            saved: None,
        };
        // `SaveChanged(True)` after the first two passes, which
        // `-quickautoclean` gates on `xeQuickCleanAutoSave`; the third pass
        // is not saved (UPSTREAM-QUIRK).
        let save = request.quick && request.quick_auto_save && !request.dry_run && pass_index < 2;
        if save && was_unsaved {
            let saved = save_file(first, request.output.clone(), false, request.backup)?;
            response.messages.push(format!("Saving: {}", first.get_name()));
            report.saved = Some(saved);
        }
        response.passes.push(report);
        if !was_unsaved {
            break;
        }
    }
    response.unsaved = targets
        .iter()
        .any(|target| target.element_base().has_state(ElementState::esUnsaved));
    // The quick clean mode ends with `mniNavLOManagersDirtyInfoClick`
    // (`if xeQuickCleanAutoSave`): the LOOT Masterlist Entries of the
    // session's dirty information, which the run's message log holds.
    if request.quick && request.quick_auto_save && !request.dry_run {
        response.messages.extend(dirty_info_lines(dirty_info));
    }
    response.dirty_info = dirty_info.clone();
    Ok(response)
}

/// The dirty information of one cleaning step (`PluginCRC32 := CRC32` of the
/// last record the walk visited, then the store of the counts): the entry of
/// that plugin and CRC32 is updated, made when missing, and the counts the
/// step does not set stay as they were. `itm` is `None` for the UDR step,
/// which sets the UDR and NAV counts.
fn store_dirty_info(infos: &mut Vec<DirtyInfo>, report: &CleanReport, itm: Option<u64>) {
    let Some((plugin, crc32)) = report.plugin.as_ref().map(|(name, crc)| (name.clone(), *crc)) else {
        return;
    };
    let entry = match infos
        .iter_mut()
        .find(|info| info.plugin == plugin && info.crc32 == crc32)
    {
        Some(entry) => entry,
        None => {
            infos.push(DirtyInfo {
                plugin,
                crc32,
                itm: 0,
                udr: 0,
                nav: 0,
            });
            infos.last_mut().expect("just pushed")
        }
    };
    match itm {
        Some(count) => entry.itm = count,
        None => {
            entry.udr = report.cleaned.len() as u64;
            entry.nav = report.deleted_navmeshes;
        }
    }
}

/// The filter for cleaning, then the UDR and the ITM removal as asked, over
/// every selected file node. The tree holds the session's files
/// (`ReInitTree`), the filter runs once (`mniNavFilterForCleaningClick`
/// before the clicks), and each click walks the selection's file nodes in
/// the order of the tree (`GetSortedSelection(True)`), each from its last
/// node back: the UDR over all of them, then the ITM removal. The file
/// nodes are collapsed (clicked, not expanded), so their children keep the
/// order of the file, unless the request says the GUI jumped to them.
fn selection_pass(
    targets: &[Arc<FileImpl>],
    loaded: &[Arc<FileImpl>],
    request: &CleanRequest,
    settings: &UdrSettings,
    allow_make_partial: bool,
) -> Result<QuickCleanPass, CommandError> {
    let mut tree = NavTree::new(loaded);
    let filter =
        apply_filter(&mut tree, &FilterOptions::for_cleaning(), loaded).map_err(|error| edit_failed(error.message))?;
    let mut starts = Vec::new();
    for target in targets {
        let start = tree
            .file_node(target)
            .ok_or_else(|| CommandError::new("internal", "the plugin has no node"))?;
        // `JumpTo` the header expands the file node, and the tree sorts the
        // children of an expanded node (`toAutoSort`); a node selected with
        // a click stays collapsed and unsorted.
        if request.jump_to {
            tree.sort_children(start);
        }
        starts.push(start);
    }
    let mut pass = QuickCleanPass {
        filter,
        ..Default::default()
    };
    if request.udr {
        pass.udr = cleaning::undelete_and_disable_references(&mut tree, &starts, request.dry_run, settings)
            .map_err(edit_failed)?;
    }
    if request.itm {
        pass.itm = cleaning::remove_identical_to_master(&mut tree, &starts, request.dry_run, allow_make_partial)
            .map_err(edit_failed)?;
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
        quick: true,
        quick_auto_save: true,
        output,
        ..Default::default()
    };
    files_clean(session, request)
}

pub(crate) fn files_clean(session: &mut Session, request: CleanRequest) -> Result<CleanResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("files.clean")?;
    let mut targets = Vec::new();
    if request.files.is_empty() {
        targets.push(session.file(request.file.as_deref())?);
    } else {
        for name in &request.files {
            targets.push(session.file(Some(name))?);
        }
    }
    for target in &targets {
        if target
            .get_file_states()
            .contains(xedit_core::interface::FileState::fsIsHardcoded)
        {
            return Err(CommandError::new(
                "not_editable",
                "the hardcoded records are not a plugin",
            ));
        }
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
    let mut dirty_info = std::mem::take(&mut session.dirty_info);
    let result =
        run_on_large_stack(|| clean_targets(&targets, &files, &request, &mut dirty_info)).and_then(|result| result);
    session.dirty_info = dirty_info;
    set_display_shorter_names(shorter_names);
    set_edit_allowed(allowed);
    result
}

#[derive(Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct DirtyInfoRequest {}

#[derive(Serialize, JsonSchema)]
pub struct DirtyInfoResponse {
    /// The dirty information the cleaning steps of the session stored
    /// (`LOOTPluginInfos`), one entry per plugin and CRC32.
    pub entries: Vec<DirtyInfo>,
    /// The lines xEdit's "LOOT Masterlist Entries" (and, for Oblivion, "BOSS
    /// Masterlist Entries") adds to the message log; empty when no step
    /// stored anything (the GUI hides the menu item then).
    pub messages: Vec<String>,
}

/// `files.dirty_info`: port of `mniNavLOManagersDirtyInfoClick`, the LOOT
/// masterlist entries of the dirty information the session's cleaning steps
/// stored.
fn files_dirty_info(session: &mut Session, _request: DirtyInfoRequest) -> Result<DirtyInfoResponse, CommandError> {
    session.mode()?;
    Ok(DirtyInfoResponse {
        entries: session.dirty_info.clone(),
        messages: dirty_info_lines(&session.dirty_info),
    })
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
        "files.dirty_info",
        "The LOOT masterlist entries of the dirty information the cleaning steps of the session stored (mniNavLOManagersDirtyInfoClick): per plugin and CRC32 the ITM, UDR and deleted navmesh counts.",
        false,
        files_dirty_info,
    );
    registry.register(
        "files.clean",
        "Clean a plugin: remove the records identical to their master (mniNavRemoveIdenticalToMasterClick), undelete and disable the deleted references (mniNavUndeleteAndDisableReferencesClick), or run the quick auto clean mode (-quickautoclean), which saves.",
        true,
        files_clean,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The report of the quick auto clean mode on Fallout 3's Anchorage.esm,
    /// as the 4.1.5q release logs it: a dirty entry from the first pass, a
    /// clean one of the saved file's CRC32 from the second (no name line
    /// again), and the reqManualFix form of a further dirty entry of the
    /// same plugin, which starts without a line break.
    #[test]
    fn writes_the_loot_entries_as_the_release() {
        let _lock = globals::test_lock();
        let app = globals::app_name();
        let url = globals::nexus_mods_url();
        globals::set_app_name("FO3");
        globals::set_nexus_mods_url("https://www.nexusmods.com/fallout3/mods/637");
        let infos = [
            DirtyInfo {
                plugin: "Anchorage.esm".to_owned(),
                crc32: 0xA4BA_9D10,
                itm: 55,
                udr: 10,
                nav: 0,
            },
            DirtyInfo {
                plugin: "Anchorage.esm".to_owned(),
                crc32: 0x2AF0_D0E8,
                itm: 0,
                udr: 0,
                nav: 0,
            },
            DirtyInfo {
                plugin: "Anchorage.esm".to_owned(),
                crc32: 0x0000_0001,
                itm: 0,
                udr: 0,
                nav: 5,
            },
        ];
        let lines = dirty_info_lines(&infos);
        globals::set_app_name(&app);
        globals::set_nexus_mods_url(&url);
        let expected = [
            "",
            "LOOT Masterlist Entries",
            "",
            "  - name: 'Anchorage.esm'",
            "    dirty:",
            "      - <<: *quickClean",
            "        crc: 0xA4BA9D10",
            "        util: '[FO3Edit v4.1.5q](https://www.nexusmods.com/fallout3/mods/637)'",
            "        itm: 55",
            "        udr: 10",
            "    clean:",
            "      - crc: 0x2AF0D0E8",
            "        util: 'FO3Edit v4.1.5q'",
            "      - <<: *reqManualFix",
            "        crc: 0x00000001",
            "        util: '[FO3Edit v4.1.5q](https://www.nexusmods.com/fallout3/mods/637)'",
            "        nav: 5",
            "",
        ];
        assert_eq!(lines, expected);
    }

    #[test]
    fn reads_the_mstt_value_as_the_dialog() {
        assert_eq!(parse_hex_def("0000001B"), Some(0x1B));
        assert_eq!(parse_hex_def("zz"), None);
        assert_eq!(parse_hex_def(""), None);
    }
}
