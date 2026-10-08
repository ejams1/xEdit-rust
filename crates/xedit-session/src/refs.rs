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
use xedit_core::interface::{Element, File, FileState, GameMode, MainRecord};

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
    let results = build_or_load_refs(files, only_load).map_err(|message| CommandError::new("refs_failed", message))?;
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
    let files = if request.file.is_some() && !request.only_load {
        // `TwbFile.BuildRef`.
        let mut results = Vec::new();
        for file in &files {
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
#[derive(Deserialize, JsonSchema)]
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
}

/// A FormID a record refers to.
#[derive(Serialize, JsonSchema)]
pub struct Reference {
    /// The FormID as the file of the record stores it.
    pub file_form_id: String,
    /// The record it resolves to, as `DoBuildRef` resolves it; none for a
    /// FormID no loaded record has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record: Option<RecordRef>,
}

/// `refs.get`: the response.
#[derive(Serialize, JsonSchema)]
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
    /// FormIDs is listed twice, as in xEdit.
    pub referenced_by: Vec<RecordRef>,
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
        referenced_by: referenced_by
            .iter()
            .skip(request.offset)
            .take(limit)
            .map(|record| record_ref(record))
            .collect(),
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
}
