// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (the reference build of the
// loader, mniNavBuildRefClick, the Referenced By tab), xEdit/xeInit.pas (the
// cache path)

//! The reference index commands: `refs.build` builds the references of the
//! loaded files or loads them from the reference cache, as the loader of the
//! GUI does after loading (`BuildOrLoadRef`) and as "Build Reference Info"
//! does for one file; `refs.get` lists the records that refer to a record
//! (the "Referenced By" tab: `ReferencedBy` of its master) and the records
//! it refers to. A command that needs the references builds them first
//! (`Session::ensure_refs`), so they are built once per session.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::refs::{BuildOrLoadRefResult, build_or_load_refs};
use xedit_core::implementation::{FileImpl, MainRecordImpl, refcache};
use xedit_core::interface::globals::{cache_path, dont_cache, set_cache_path};
use xedit_core::interface::{
    CopyArgs, Element, ElementRef, File, FileState, GameMode, MainRecord, MainRecordRef, Signature,
};

use crate::edit::with_edit_allowed;
use crate::formids::{RecordRef, record_ref};
use crate::{CommandError, Registry, Session};

/// Port of the cache switches of `xeInit`: `-C:<path>` (`path`),
/// `-DontCache`, `-DontCacheLoad` and `-DontCacheSave`. Set before the
/// plugins load; [`init_cache_path`] completes the path once they are.
pub fn set_cache_options(path: Option<&str>, no_cache: bool, no_load: bool, no_save: bool) {
    use xedit_core::interface::globals::{set_dont_cache, set_dont_cache_load, set_dont_cache_save};
    set_cache_path(path.unwrap_or(""));
    set_dont_cache(no_cache);
    set_dont_cache_load(no_cache || no_load);
    set_dont_cache_save(no_cache || no_save);
    if (no_cache || no_load) && (no_cache || no_save) {
        set_dont_cache(true);
    }
}

/// Port of the cache path of `xeInit`: the path of `-C:`, else
/// `<AppName>Edit Cache` in the data folder of the loaded plugins; empty
/// when the cache is off (`-DontCache`). Called once the plugins are
/// loaded; the folder is made when the first cache file is written.
pub fn init_cache_path() {
    if xedit_core::interface::globals::game_mode() == GameMode::gmTES3 {
        // `xeInit` turns the cache off for Morrowind.
        set_cache_options(None, true, true, true);
    }
    if dont_cache() {
        set_cache_path("");
        return;
    }
    let mut path = cache_path();
    if path.is_empty() {
        path = refcache::default_cache_path();
    } else if !path.ends_with(['\\', '/']) {
        path.push('\\');
    }
    set_cache_path(&path);
}

/// The files the loader of the GUI builds the references of: the hardcoded
/// file and every module (`(fsIsHardcoded in FileStates) or not
/// IsNotPlugin`), in load order.
fn ref_files() -> Vec<std::sync::Arc<FileImpl>> {
    let mut files: Vec<_> = xedit_core::implementation::masters::loaded_files()
        .into_iter()
        .filter(|file| file.get_file_states().contains(FileState::fsIsHardcoded) || !file.get_is_not_plugin())
        .collect();
    files.sort_by_key(|file| file.load_order());
    files
}

/// The result of the reference build of one file.
#[derive(Serialize, JsonSchema, Clone)]
pub struct FileRefs {
    pub file: String,
    /// `built`, `built_and_saved` (built and written to the cache),
    /// `loaded` (read from the cache) or `none`.
    pub result: String,
    /// The line the loader of xEdit logs for the file.
    pub message: String,
    /// The cache file of the file, when the cache is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_file: Option<String>,
}

fn result_name(result: BuildOrLoadRefResult) -> &'static str {
    match result {
        BuildOrLoadRefResult::blrNone => "none",
        BuildOrLoadRefResult::blrBuilt => "built",
        BuildOrLoadRefResult::blrBuiltAndSaved => "built_and_saved",
        BuildOrLoadRefResult::blrLoaded => "loaded",
    }
}

impl Session {
    /// Builds or loads the references of every loaded file that has none
    /// yet, as the loader of the GUI does once the files are loaded.
    pub fn ensure_refs(&mut self) -> Result<Vec<FileRefs>, CommandError> {
        if self.mode()? == GameMode::gmTES3 {
            // `wbBuildRefs` is off for Morrowind: the loader builds none.
            return Ok(Vec::new());
        }
        let files: Vec<_> = ref_files().into_iter().filter(|file| !file.refs_built()).collect();
        build(&files, false)
    }
}

/// `BuildOrLoadRef` of `files`, reported per file.
fn build(files: &[std::sync::Arc<FileImpl>], only_load: bool) -> Result<Vec<FileRefs>, CommandError> {
    let results = build_or_load_refs(files, only_load).map_err(|message| {
        // `cancelled:` is the wait form's stop (`GeneralProgress`), not a
        // failure of the build.
        if message.starts_with("cancelled") {
            CommandError::new("cancelled", "the operation was cancelled")
        } else {
            CommandError::new("refs_failed", message)
        }
    })?;
    Ok(files
        .iter()
        .zip(results)
        .map(|(file, result)| FileRefs {
            file: file.get_name(),
            result: result_name(result).to_owned(),
            message: format!("[{}] {}", file.get_name(), result.message(only_load)),
            cache_file: refcache::cache_file_name(file).map(|path| path.display().to_string()),
        })
        .collect())
}

/// `refs.build`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefsBuildRequest {
    /// Only this loaded file ("Build Reference Info" of one file:
    /// `TwbFile.BuildRef`, which also builds the references of the records
    /// that changed since); every loaded file when omitted.
    pub file: Option<String>,
    /// Only load the references from the cache; build none
    /// (`BuildOrLoadRef(True)`).
    #[serde(default)]
    pub only_load: bool,
}

/// `refs.build`: the response.
#[derive(Serialize, JsonSchema)]
pub struct RefsBuildResponse {
    /// The folder of the cache files; empty when the cache is off.
    pub cache_path: String,
    pub files: Vec<FileRefs>,
}

fn refs_build(session: &mut Session, request: RefsBuildRequest) -> Result<RefsBuildResponse, CommandError> {
    let files = match &request.file {
        Some(name) => vec![session.file(Some(name)).or_else(|_| {
            ref_files()
                .into_iter()
                .find(|file| file.get_name().eq_ignore_ascii_case(name))
                .ok_or_else(|| CommandError::new("unknown_file", format!("{name} is not loaded")))
        })?],
        None => {
            session.mode()?;
            ref_files()
        }
    };
    // `GeneralProgress`: the operation stops at its next poll point once the
    // wait form was cancelled.
    xedit_core::interface::wait_form::poll_cancel()
        .map_err(|_| CommandError::new("cancelled", "the operation was cancelled"))?;
    let files = if request.file.is_some() && !request.only_load {
        // `TwbFile.BuildRef`.
        let mut results = Vec::new();
        for file in &files {
            xedit_core::interface::wait_form::poll_cancel()
                .map_err(|_| CommandError::new("cancelled", "the operation was cancelled"))?;
            let result = file
                .build_ref()
                .map_err(|message| CommandError::new("refs_failed", message))?;
            results.push(FileRefs {
                file: file.get_name(),
                result: result_name(result).to_owned(),
                message: format!("[{}] {}", file.get_name(), result.message(false)),
                cache_file: refcache::cache_file_name(file).map(|path| path.display().to_string()),
            });
        }
        results
    } else {
        build(&files, request.only_load)?
    };
    Ok(RefsBuildResponse {
        cache_path: cache_path(),
        files,
    })
}

/// `refs.get`: the request.
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefsGetRequest {
    /// Load order FormID of the record as hexadecimal digits.
    pub form_id: String,
    /// Plugin whose version of the record to read; the last loaded plugin
    /// when omitted. The referenced-by list is the master's either way.
    pub file: Option<String>,
    /// Entries of the referenced-by list to skip.
    #[serde(default)]
    pub offset: usize,
    /// Entries of the referenced-by list to return at most; all when
    /// omitted.
    pub limit: Option<usize>,
    /// `edReferencedByFilterName`: keep the entries whose name contains this
    /// text, without regard to case.
    pub filter_name: Option<String>,
    /// `edReferencedByFilterSignature`.
    pub filter_signature: Option<String>,
    /// `edReferencedByFilterFileName`.
    pub filter_file: Option<String>,
    /// `cobReferencedByFilter` at 1: the name or the signature may match
    /// instead of both (the file always has to match).
    #[serde(default)]
    pub filter_or: bool,
}

/// A FormID a record refers to.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct Reference {
    /// The FormID as the file of the record stores it.
    pub file_form_id: String,
    /// The record it resolves to, as `DoBuildRef` resolves it; none for a
    /// FormID no loaded record has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<RecordRef>,
}

/// `refs.get`: the response.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct RefsGetResponse {
    /// The record as `file` sees it.
    pub record: RecordRef,
    /// Its master (`MasterOrSelf`), which keeps the list.
    pub master: RecordRef,
    /// `ReferencedByCount`.
    pub referenced_by_count: usize,
    /// `ReferencedBy` of the master: the records that refer to the record or
    /// one of its overrides, by load order FormID and then by the load order
    /// of their file, from `offset`. A record that refers to it through two
    /// FormIDs is listed twice, as in xEdit. With a filter it holds the
    /// entries that match (`ApplyReferencedByFilter`).
    pub referenced_by: Vec<RecordRef>,
    /// `ReferencedByCount`: the entries before the filter (`N` of the tab
    /// caption `Referenced by (N / F: M)`).
    pub filtered_count: usize,
    /// `References` of the record itself: the FormIDs its elements refer
    /// to, sorted as stored.
    pub references: Vec<Reference>,
}

fn refs_get(session: &mut Session, request: RefsGetRequest) -> Result<RefsGetResponse, CommandError> {
    let record = session.record(&request.form_id, request.file.as_deref())?;
    session.ensure_refs()?;
    let record: std::sync::Arc<MainRecordImpl> = crate::formids::record_impl(&record)?;
    let master = record.master_or_self_impl();
    let referenced_by = master.referenced_by();
    let count = referenced_by.len();
    // `ApplyReferencedByFilter`: the name, signature and file of an entry as
    // the tab shows them, lower case, the name and signature joined by AND
    // or OR, the file always.
    let name_filter = request
        .filter_name
        .as_deref()
        .map(|text| text.trim().to_ascii_lowercase());
    let signature_filter = request
        .filter_signature
        .as_deref()
        .map(|text| text.trim().to_ascii_lowercase());
    let file_filter = request
        .filter_file
        .as_deref()
        .map(|text| text.trim().to_ascii_lowercase());
    let use_name = name_filter.as_ref().is_some_and(|text| !text.is_empty());
    let use_signature = signature_filter.as_ref().is_some_and(|text| !text.is_empty());
    let use_file = file_filter.as_ref().is_some_and(|text| !text.is_empty());
    let matching = |record: &std::sync::Arc<MainRecordImpl>| {
        let name = record.get_name().to_lowercase();
        let signature = record.get_signature().to_string().to_lowercase();
        let file = record
            .get_file()
            .map(|file| file.get_name().to_lowercase())
            .unwrap_or_default();
        let name_matches = !use_name || name.contains(name_filter.as_deref().unwrap_or_default());
        let signature_matches = !use_signature || signature.contains(signature_filter.as_deref().unwrap_or_default());
        let file_matches = !use_file || file.contains(file_filter.as_deref().unwrap_or_default());
        file_matches
            && if request.filter_or {
                name_matches || signature_matches
            } else {
                name_matches && signature_matches
            }
    };
    let filtered: Vec<_> = if use_name || use_signature || use_file {
        referenced_by
            .iter()
            .filter(|record| matching(record))
            .cloned()
            .collect()
    } else {
        referenced_by.clone()
    };
    let filtered_count = filtered.len();
    let limit = request.limit.unwrap_or(usize::MAX);
    let references = record
        .references()
        .into_iter()
        .map(|form_id| Reference {
            file_form_id: form_id.to_string(false),
            record: record.resolve_reference(form_id).map(|target| record_ref(&target)),
        })
        .collect();
    Ok(RefsGetResponse {
        record: record_ref(&record),
        master: record_ref(&master),
        referenced_by_count: count,
        referenced_by: filtered
            .iter()
            .skip(request.offset)
            .take(limit)
            .map(|record| record_ref(record))
            .collect(),
        filtered_count,
        references,
    })
}

/// Writes the referenced-by lists of every loaded file as text, for the
/// parity check against the oracle (`xedit refs dump`, the counterpart of
/// `crates/xtask/oracle/refs.pas`): a line `F <name>` per loaded file in
/// load order, then for every master record with a non-empty list, file by
/// file in the order of `flRecords`, a line `R <load order FormID>@<file>
/// <count>` and the entries as `|<load order FormID>@<file>`, 32 to a line.
pub fn write_index(session: &mut Session, out: &mut dyn std::io::Write) -> Result<(), CommandError> {
    session.ensure_refs()?;
    let io = |error: std::io::Error| CommandError::new("io", error.to_string());
    let mut files = xedit_core::implementation::masters::loaded_files();
    files.sort_by_key(|file| file.load_order());
    for file in &files {
        writeln!(out, "F {}", file.get_name()).map_err(io)?;
    }
    for file in &files {
        for record in file.records() {
            if record.master().is_some() {
                continue;
            }
            let referenced_by = record.referenced_by();
            if referenced_by.is_empty() {
                continue;
            }
            writeln!(
                out,
                "R {}@{} {}",
                record.get_load_order_form_id().to_string(false),
                file.get_name(),
                referenced_by.len()
            )
            .map_err(io)?;
            for chunk in referenced_by.chunks(32) {
                let mut line = String::new();
                for entry in chunk {
                    let name = entry.record_file().map(|file| file.get_name()).unwrap_or_default();
                    line.push_str(&format!("|{}@{name}", entry.get_load_order_form_id().to_string(false)));
                }
                writeln!(out, "{line}").map_err(io)?;
            }
        }
    }
    Ok(())
}

/// `references.change`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferencesChangeRequest {
    /// Load order FormID of the referenced record as hexadecimal digits
    /// (`ActiveMaster.LoadOrderFormID` of the change).
    pub form_id: String,
    /// Plugin whose version of the record names the master the referencing
    /// records of which are updated; the last loaded plugin when omitted.
    pub file: Option<String>,
    /// The new load order FormID, as the dialog asks it.
    pub new_form_id: String,
    /// Only the referencing records of these loaded files, as the file
    /// selection dialog checks them; every editable referencing record when
    /// omitted.
    #[serde(default)]
    pub files: Vec<String>,
    /// Report the records, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `references.change`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ReferencesChangeResponse {
    /// The referenced record as `file` sees it.
    pub record: RecordRef,
    /// Its master, which keeps the referenced-by list.
    pub master: RecordRef,
    /// `ReferencedByCount`: the entries before the file selection.
    pub referenced_by_count: usize,
    /// The editable referencing records of the selection.
    pub records: Vec<RecordRef>,
    /// How many of them now refer to the new FormID.
    pub updated: usize,
    /// The errors upstream logs and goes on after.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    /// The new FormID, as eight hexadecimal digits.
    pub new_form_id: String,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// `references.change`: change the references in the selected referencing
/// records from one FormID to another (`mniNavChangeReferencingRecords`):
/// every record that refers to the record gets `CompareExchangeFormID`
/// with the two load order FormIDs. The record itself keeps its FormID,
/// unlike `formids.change`.
fn references_change(
    session: &mut Session,
    request: ReferencesChangeRequest,
) -> Result<ReferencesChangeResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("references.change")?;
    let old_form_id = crate::commands::parse_form_id(&request.form_id)?;
    let new_form_id = crate::commands::parse_form_id(&request.new_form_id)?;
    if new_form_id.is_null() {
        return Err(CommandError::new("invalid_params", "00000000 is not a valid FormID"));
    }
    if new_form_id.is_player() {
        return Err(CommandError::new("invalid_params", "00000014 is not a valid FormID"));
    }
    if new_form_id == old_form_id {
        return Err(CommandError::new("invalid_params", "Old and new FormID are identical"));
    }
    let record = session.record(&request.form_id, request.file.as_deref())?;
    session.ensure_refs()?;
    let record: std::sync::Arc<MainRecordImpl> = crate::formids::record_impl(&record)?;
    let master = record.master_or_self_impl();
    let referenced_by = master.referenced_by();
    let selected: Vec<&std::sync::Arc<MainRecordImpl>> = referenced_by
        .iter()
        .filter(|referrer| {
            request.files.is_empty()
                || referrer.get_file().is_some_and(|file| {
                    request
                        .files
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(&file.get_name()))
                })
        })
        .filter(|referrer| with_edit_allowed(|| crate::formids::record_editable(referrer)))
        .collect();
    let mut errors = Vec::new();
    let mut updated = 0;
    for referrer in &selected {
        // `ShowChangeReferencedBy` updates the records in a long action and
        // ticks for every one of them, which `GeneralProgress` turns into an
        // `Abort` when the user cancelled. The check is before the record is
        // updated, so a cancelled update leaves the records after it as they
        // were.
        xedit_core::interface::wait_form::poll_cancel()
            .map_err(|_| CommandError::new("cancelled", "the operation was cancelled"))?;
        if request.dry_run {
            continue;
        }
        match referrer.compare_exchange_form_id(old_form_id, new_form_id) {
            Ok(true) => updated += 1,
            Ok(false) => {}
            Err(error) => errors.push(format!("Error updating FormID for {}: {error}", referrer.get_name())),
        }
    }
    Ok(ReferencesChangeResponse {
        record: record_ref(&record),
        master: record_ref(&master),
        referenced_by_count: referenced_by.len(),
        records: selected.iter().map(|referrer| record_ref(referrer)).collect(),
        updated,
        errors,
        new_form_id: new_form_id.to_string(false),
        dry_run: request.dry_run,
    })
}

/// `references.change_batch`: one pair of the batch.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferencePair {
    /// Load order FormID of the old record, as the CSV's old master and
    /// object ID name it.
    pub old_form_id: String,
    /// The old master file, as the CSV names it; the version of the record
    /// in that file is the one whose references change. Omitted for the
    /// winning override.
    pub old_file: Option<String>,
    /// Load order FormID of the new record.
    pub new_form_id: String,
    /// The new master file, as the CSV names it; omitted for the winning
    /// override.
    pub new_file: Option<String>,
}

/// `references.change_batch`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferencesChangeBatchRequest {
    /// The loaded plugin whose referencing records are updated, as the
    /// focused file node of the batch item; the only loaded plugin when
    /// omitted.
    pub file: Option<String>,
    /// The pairs, as the lines of the CSV the dialog opens.
    pub pairs: Vec<ReferencePair>,
    /// Report the records, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// What `references.change_batch` did with one pair.
#[derive(Serialize, JsonSchema)]
pub struct BatchChangeResult {
    /// Load order FormID of the old record.
    pub old_form_id: String,
    /// Load order FormID of the new record.
    pub new_form_id: String,
    /// The referencing records of the file that were updated.
    pub records: Vec<RecordRef>,
    /// How many of them now refer to the new FormID.
    pub updated: usize,
    /// Whether the file's own version of the old record took the new
    /// FormID.
    pub renamed: bool,
    /// Why the pair was skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

/// `references.change_batch`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ReferencesChangeBatchResponse {
    /// The plugin whose records were updated.
    pub file: String,
    pub pairs: Vec<BatchChangeResult>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// `references.change_batch`: the batch form of `references.change`
/// (`mniNavBatchChangeReferencingRecords`): for every pair the referencing
/// records of the focused file get `CompareExchangeFormID`, the new
/// record's file becomes a master of the focused file, and the focused
/// file's version of the old record takes the new FormID. The CSV's
/// signature and editor ID checks are warnings of the dialog and have no
/// data effect.
fn references_change_batch(
    session: &mut Session,
    request: ReferencesChangeBatchRequest,
) -> Result<ReferencesChangeBatchResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("references.change_batch")?;
    let target = session.file(request.file.as_deref())?;
    session.ensure_refs()?;
    let mut results = Vec::new();
    for pair in &request.pairs {
        // The batch changes the referencing records pair by pair, and every
        // pair is a `ShowChangeReferencedBy` of the release, which aborts
        // when the user cancelled. The check is before the pair is worked
        // out, so a cancelled batch leaves the pairs after it as they were.
        xedit_core::interface::wait_form::poll_cancel()
            .map_err(|_| CommandError::new("cancelled", "the operation was cancelled"))?;
        let old_form_id = crate::commands::parse_form_id(&pair.old_form_id)?;
        let new_form_id = crate::commands::parse_form_id(&pair.new_form_id)?;
        let skipped = |reason: String| BatchChangeResult {
            old_form_id: old_form_id.to_string(false),
            new_form_id: new_form_id.to_string(false),
            records: Vec::new(),
            updated: 0,
            renamed: false,
            skipped: Some(reason),
        };
        let old = session
            .record(&pair.old_form_id, pair.old_file.as_deref())
            .and_then(|record| crate::formids::record_impl(&record));
        let Ok(old) = old else {
            results.push(skipped(format!(
                "Old Record with FormID {} was not found",
                old_form_id.to_string(false)
            )));
            continue;
        };
        let new = session
            .record(&pair.new_form_id, pair.new_file.as_deref())
            .and_then(|record| crate::formids::record_impl(&record));
        let Ok(new) = new else {
            results.push(skipped(format!(
                "New Record with FormID {} was not found",
                new_form_id.to_string(false)
            )));
            continue;
        };
        if old.get_element_id() == new.get_element_id() {
            results.push(skipped("Old and new Record are identical".to_owned()));
            continue;
        }
        let referenced_by: Vec<std::sync::Arc<MainRecordImpl>> = old
            .referenced_by()
            .into_iter()
            .filter(|referrer| {
                referrer
                    .get_file()
                    .is_some_and(|file| file.get_name().eq_ignore_ascii_case(&target.get_name()))
            })
            .collect();
        if referenced_by.is_empty() {
            results.push(skipped(format!(
                "Old Record \"{}\" is not referenced by any record in file \"{}\"",
                old.get_name(),
                target.get_name()
            )));
            continue;
        }
        let mut updated = 0;
        if !request.dry_run {
            if let Some(new_file) = new.get_file()
                && !new_file.get_name().eq_ignore_ascii_case(&target.get_name())
            {
                target
                    .add_master_if_missing(&new_file.get_name(), true, true)
                    .map_err(|error| CommandError::new("edit_failed", error))?;
            }
            for referrer in &referenced_by {
                // The check is before the record is updated, as in
                // `references.change`.
                xedit_core::interface::wait_form::poll_cancel()
                    .map_err(|_| CommandError::new("cancelled", "the operation was cancelled"))?;
                if with_edit_allowed(|| crate::formids::record_editable(referrer))
                    && referrer
                        .compare_exchange_form_id(old_form_id, new_form_id)
                        .map_err(|error| CommandError::new("edit_failed", error))?
                {
                    updated += 1;
                }
            }
            if let Some(record) = target.contained_record_by_load_order_form_id(old_form_id) {
                record
                    .set_load_order_form_id(new_form_id)
                    .map_err(|error| CommandError::new("edit_failed", error))?;
            }
        }
        let renamed = !request.dry_run && target.contained_record_by_load_order_form_id(old_form_id).is_none();
        results.push(BatchChangeResult {
            old_form_id: old_form_id.to_string(false),
            new_form_id: new_form_id.to_string(false),
            records: referenced_by.iter().map(|referrer| record_ref(referrer)).collect(),
            updated,
            renamed,
            skipped: None,
        });
    }
    Ok(ReferencesChangeBatchResponse {
        file: target.get_name(),
        pairs: results,
        dry_run: request.dry_run,
    })
}

/// Port of `IsExterior`: the element is below a worldspace exterior block
/// or an exterior cell block, sub-block or cell.
fn is_exterior(element: &ElementRef) -> bool {
    let mut current = Some(element.clone());
    while let Some(element) = current {
        if let Some(group) = element
            .as_element_impl()
            .and_then(|element| element.group_record_impl())
        {
            match group.group_type() {
                0 => return group.group_label() == u32::from_le_bytes(*b"WRLD"),
                1 | 4 | 5 => return true,
                2 | 3 => return false,
                _ => {}
            }
        }
        current = element.get_container();
    }
    false
}

/// The `NAME` record the reference points at.
fn base_record(record: &MainRecordRef) -> Option<MainRecordRef> {
    record
        .as_container()?
        .get_element_by_signature(Signature::new(b"NAME"))?
        .get_links_to()?
        .into_main_record()
}

/// `records` of the selected files and the selected records, each once.
fn selection_records(
    session: &Session,
    files: &[String],
    records: &[String],
) -> Result<Vec<MainRecordRef>, CommandError> {
    let mut selected: Vec<MainRecordRef> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |record: MainRecordRef| {
        if seen.insert(record.get_element_id()) {
            selected.push(record);
        }
    };
    for name in files {
        let file = session.file(Some(name))?;
        for record in file.records() {
            push(record as MainRecordRef);
        }
    }
    for form_id in records {
        push(session.record(form_id, None)?);
    }
    Ok(selected)
}

/// The references a VWD auto command changes: `REFR`, exterior, with a
/// `NAME` that points at a base record with a `_far.nif`, and not visible
/// when distant yet.
fn vwd_candidates(records: &[MainRecordRef], check_errors: bool) -> Vec<(&MainRecordRef, String)> {
    let mut candidates = Vec::new();
    for record in records {
        if record.get_signature().to_string() != "REFR" {
            continue;
        }
        if record.get_is_visible_when_distant() {
            continue;
        }
        let record_ref: ElementRef = record.clone();
        if !is_exterior(&record_ref) {
            continue;
        }
        let Some(base) = base_record(record) else {
            continue;
        };
        if !base.get_has_visible_when_distant_mesh() {
            continue;
        }
        if check_errors && !record.get_check().is_empty() {
            continue;
        }
        candidates.push((record, base.get_name()));
    }
    candidates
}

/// `references.set_vwd_auto`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetVwdAutoRequest {
    /// The selected file nodes: every reference of them is examined.
    #[serde(default)]
    pub files: Vec<String>,
    /// The selected record nodes, as load order FormIDs.
    #[serde(default)]
    pub records: Vec<String>,
    /// Report the references, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// A reference a VWD auto command changed or skipped.
#[derive(Serialize, JsonSchema)]
pub struct VwdRecord {
    #[serde(flatten)]
    pub record: RecordRef,
    /// The base record's name, for the skip messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Why the reference was skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `references.set_vwd_auto`: the response.
#[derive(Serialize, JsonSchema)]
pub struct SetVwdAutoResponse {
    /// The references that were (or would be) changed.
    pub changed: Vec<VwdRecord>,
    /// The references that were not, with the reason.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<VwdRecord>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// `references.set_vwd_auto`: `IsVisibleWhenDistant := True` for every
/// exterior reference of the selection whose base record has a visible when
/// distant mesh (`mniNavSetVWDAuto`). A reference that is not editable is
/// reported and left as it is, as the item messages "Can't change".
fn references_set_vwd_auto(
    session: &mut Session,
    request: SetVwdAutoRequest,
) -> Result<SetVwdAutoResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("references.set_vwd_auto")?;
    let records = selection_records(session, &request.files, &request.records)?;
    let candidates = vwd_candidates(&records, false);
    let mut changed = Vec::new();
    let mut skipped = Vec::new();
    for (record, base) in candidates {
        // UPSTREAM-QUIRK: the release's item only updates its caption every
        // 500 ms and never checks the cancel, so the check is the port's own.
        // It is before the record is changed, so a cancelled command leaves
        // the references after it as they were.
        xedit_core::interface::wait_form::poll_cancel()
            .map_err(|_| CommandError::new("cancelled", "the operation was cancelled"))?;
        if !with_edit_allowed(|| record.get_is_editable()) {
            skipped.push(VwdRecord {
                record: record_ref_of(record),
                base: Some(base),
                reason: Some("Can't change: the reference is not editable".to_owned()),
            });
            continue;
        }
        if !request.dry_run {
            record.set_is_visible_when_distant(true);
        }
        changed.push(VwdRecord {
            record: record_ref_of(record),
            base: Some(base),
            reason: None,
        });
    }
    Ok(SetVwdAutoResponse {
        changed,
        skipped,
        dry_run: request.dry_run,
    })
}

/// `references.copy_vwd_auto`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CopyVwdAutoRequest {
    /// The selected file nodes: every reference of them is examined.
    #[serde(default)]
    pub files: Vec<String>,
    /// The selected record nodes, as load order FormIDs.
    #[serde(default)]
    pub records: Vec<String>,
    /// Plugin to copy the references into as overrides.
    pub to: String,
    /// Report the copies, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `references.copy_vwd_auto`: the response.
#[derive(Serialize, JsonSchema)]
pub struct CopyVwdAutoResponse {
    /// The copies that were (or would be) made, with the Visible When
    /// Distant flag set.
    pub copied: Vec<VwdRecord>,
    /// The references that were not copied, with the reason.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<VwdRecord>,
    /// The plugin copied into.
    pub file: String,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// `references.copy_vwd_auto`: the "into" form of the item
/// (`mniNavSetVWDAutoInto`): the exterior references of the selection whose
/// base record has a visible when distant mesh are copied into the chosen
/// plugin as overrides, and each copy gets the Visible When Distant flag
/// (`CopyInto` with `SetVWDCallback`). A reference whose check reports an
/// error is skipped, as `MainRecord.HasErrors` skips it in the release.
fn references_copy_vwd_auto(
    session: &mut Session,
    request: CopyVwdAutoRequest,
) -> Result<CopyVwdAutoResponse, CommandError> {
    crate::commands::refuse_in_translate_mode("references.copy_vwd_auto")?;
    let records = selection_records(session, &request.files, &request.records)?;
    let target = session.file(Some(&request.to))?;
    if !with_edit_allowed(|| target.get_is_editable()) {
        return Err(CommandError::new(
            "not_editable",
            format!("File \"{}\" is not editable", target.get_name()),
        ));
    }
    let mut copied = Vec::new();
    let mut skipped = Vec::new();
    for (record, base) in vwd_candidates(&records, true) {
        // `CopyInto` copies the elements of a record in a long action and
        // reports every element it copies, which `GeneralProgress` turns
        // into an `Abort` when the user cancelled. The check is before the
        // reference is copied, so a cancelled command leaves the references
        // after it uncopied.
        xedit_core::interface::wait_form::poll_cancel()
            .map_err(|_| CommandError::new("cancelled", "the operation was cancelled"))?;
        let source: ElementRef = record.clone();
        let required = xedit_core::implementation::copy::required_masters(&source, false);
        if let Err(error) = xedit_core::implementation::copy::missing_masters(&required, &target) {
            skipped.push(VwdRecord {
                record: record_ref_of(record),
                base: Some(base),
                reason: Some(error),
            });
            continue;
        }
        if request.dry_run {
            copied.push(VwdRecord {
                record: record_ref_of(record),
                base: Some(base),
                reason: None,
            });
            continue;
        }
        if let Err(error) = xedit_core::implementation::copy::add_required_masters_of(&required, &target) {
            skipped.push(VwdRecord {
                record: record_ref_of(record),
                base: Some(base),
                reason: Some(error),
            });
            continue;
        }
        let args = CopyArgs {
            as_new: false,
            deep_copy: false,
            ..CopyArgs::default()
        };
        match xedit_core::implementation::copy::copy_into_file(&source, &target, &args) {
            Ok(Some(copy)) => {
                let copy: MainRecordRef = copy.into_main_record().unwrap_or_else(|| record.clone());
                copy.set_is_visible_when_distant(true);
                copied.push(VwdRecord {
                    record: record_ref_of(&copy),
                    base: Some(base),
                    reason: None,
                });
            }
            Ok(None) => skipped.push(VwdRecord {
                record: record_ref_of(record),
                base: Some(base),
                reason: Some("the copy gave no element".to_owned()),
            }),
            Err(error) => skipped.push(VwdRecord {
                record: record_ref_of(record),
                base: Some(base),
                reason: Some(error),
            }),
        }
    }
    Ok(CopyVwdAutoResponse {
        copied,
        skipped,
        file: target.get_name(),
        dry_run: request.dry_run,
    })
}

/// The summary of a record read through the main record interface.
fn record_ref_of(record: &MainRecordRef) -> RecordRef {
    match crate::formids::record_impl(record) {
        Ok(record) => record_ref(&record),
        Err(_) => RecordRef {
            form_id: record.get_load_order_form_id().to_string(false),
            signature: record.get_signature().to_string(),
            editor_id: record.get_editor_id(),
            name: record.get_name(),
            file: record.get_file().map(|file| file.get_name()).unwrap_or_default(),
        },
    }
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "refs.build",
        "Build the reference information of the loaded files, or load it from the reference cache (BuildOrLoadRef).",
        false,
        refs_build,
    );
    registry.register(
        "refs.get",
        "The records that refer to a record (ReferencedBy of its master) and the records it refers to.",
        false,
        refs_get,
    );
    registry.register(
        "references.set_vwd_auto",
        "Set Visible When Distant on every exterior reference of the selection whose base has a VWD mesh (mniNavSetVWDAuto).",
        true,
        references_set_vwd_auto,
    );
    registry.register(
        "references.copy_vwd_auto",
        "Copy the exterior references of the selection with a VWD mesh into a plugin as overrides with Visible When Distant set (mniNavSetVWDAutoInto).",
        true,
        references_copy_vwd_auto,
    );
    registry.register(
        "references.change",
        "Change the references in the referencing records from one FormID to another, without changing the record's FormID (mniNavChangeReferencingRecords).",
        true,
        references_change,
    );
    registry.register(
        "references.change_batch",
        "Change the references of the focused plugin's records for a list of FormID pairs and rename its old-record overrides (mniNavBatchChangeReferencingRecords).",
        true,
        references_change_batch,
    );
}
