// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xeScriptForm.pas (ReadScriptsList), xeInit.pas
// (wbScriptsPath) and xEdit/JvI/xejviScriptHost.pas (the scripts folder the
// units of a script's `uses` resolve from); xeMainForm.pas (DoRunScript,
// ApplyScript, ApplyScriptToSelection, ScriptCanProcessElement,
// ScriptProcessSingleElement, vstNavInitNode/vstNavInitChildren's element
// children, SaveChanged at the end of an auto mode, wbFormatElapsedTime) and
// xEdit/JvI/xejviScriptHost.pas (the OnSetValue script variables and the
// OnStatement tick) and xEdit/JvI/xejviScriptAdapterHost.pas with
// xejviScriptAdapterFile.pas/xejviScriptAdapterElement.pas (the session-side
// registrations a script of the corpus needs: the message log, the loaded
// files, the paths, the filter) and xEdit/JvI/xejviScriptAdapter.pas (the
// `et*`, `ca*` and `ct*` constants the filter hooks read)

//! The commands of phase 6: `script.list` lists the Pascal scripts of the
//! scripts folder, the corpus `script.check` compiles against and `script.run`
//! runs. `script.check` compiles the given scripts and the units they
//! `uses` (the front end of `crates/xedit-script`, phase 6 step 2); only
//! syntax is checked -- unresolved identifiers and the type semantics of the
//! interpreter belong to `script.run` (phase 6 steps 3 to 5) -- and, as
//! `TJvInterpreterUnit.Compile` does, a routine body is only scanned for its
//! balanced `end`, so a statement-level error surfaces when the script runs.
//! The folder is upstream's `wbScriptsPath`: the `--scripts` folder, else
//! `XEDIT_SCRIPTS`, else the oracle's `Edit Scripts`.
//!
//! `script.run` is the port of the `-script:` mode: `DoRunScript` and
//! `ApplyScript` of `xeMainForm.pas` over the host of `crates/xedit-script`
//! (`Script`). It loads nothing of its own -- the plugins come from `--load`
//! as for every other command -- and it *writes nothing* unless a plugin it
//! changed is saved: see [`ScriptRunRequest`].

// The interpreter's error (`ExecError`) is a value the run loop moves
// around; boxing it would hide the structure the port mirrors.
#![allow(clippy::result_large_err)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::{ElementImpl, ElementState};
use xedit_core::interface::globals;
use xedit_core::interface::{
    ConflictAll, ConflictThis, Element, ElementRef, ElementType, ElementTypes, FileState, PascalEnum, Signature,
};
use xedit_io::collate::ansi_compare_text;
use xedit_script::adapter::{Adapter, Args};
use xedit_script::error::{IE_DIRECT_INVALID_ARGUMENT, IE_TYPE_MISMATCH, IE_USER_BREAK};
use xedit_script::host::Script;
use xedit_script::values::{
    ClassDef, ParamType, ScriptObject, Value, ValueError, VarType, to_bool, to_i64, to_str, v2s,
};
use xedit_script::{ExecError, Interpreter};

use crate::{CommandError, Registry, Session};

/// `script.list`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptListRequest {
    /// The folder with the `*.pas` scripts: the `Edit Scripts` folder. When
    /// omitted, the `XEDIT_SCRIPTS` environment variable, else the oracle's
    /// `Edit Scripts` (the `XEDIT_ORACLE_DIR` folder), else the `Edit
    /// Scripts` folder beside this program.
    pub scripts: Option<String>,
}

/// One script of the folder.
#[derive(Serialize, JsonSchema)]
pub struct ScriptInfo {
    /// File name of the script, `<Unit>.pas`: a script's unit name matches
    /// its file name, so the file name is the unit a `uses` of another
    /// script resolves through the folder (`_newscript_.pas`, the form's
    /// template, is the one exception).
    pub name: String,
    /// Lines of the file.
    pub lines: u32,
}

/// `script.list`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ScriptListResponse {
    /// The folder that was listed.
    pub folder: String,
    /// Every `*.pas` script of the folder, by name.
    pub scripts: Vec<ScriptInfo>,
}

/// The folder the scripts are read from: the one named by the caller, else
/// `XEDIT_SCRIPTS`, else the oracle's `Edit Scripts`, else this program's
/// (upstream's `wbScriptsPath` default of `xeInit.pas`, which `-S:` sets).
pub fn scripts_folder(explicit: Option<&str>) -> PathBuf {
    if let Some(folder) = explicit {
        return PathBuf::from(folder);
    }
    if let Some(folder) = std::env::var_os("XEDIT_SCRIPTS") {
        return PathBuf::from(folder);
    }
    if let Some(oracle) = std::env::var_os("XEDIT_ORACLE_DIR") {
        return PathBuf::from(oracle).join("Edit Scripts");
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.join("Edit Scripts")))
        .unwrap_or_else(|| PathBuf::from("Edit Scripts"))
}

/// Lists the `*.pas` scripts of `folder` as `ReadScriptsList` fills the
/// script combo box: the top folder only, sorted as a `TStringList`. Unlike
/// the combo box, the `_newscript_` placeholder of the form is listed too:
/// it is one of the shipped scripts of the corpus (`script.check` parses
/// every file of the folder).
pub fn list_scripts(folder: &Path) -> Result<Vec<ScriptInfo>, CommandError> {
    let entries =
        std::fs::read_dir(folder).map_err(|error| CommandError::new("io", format!("{}: {error}", folder.display())))?;
    let mut scripts = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| CommandError::new("io", format!("{}: {error}", folder.display())))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pas"))
        {
            continue;
        }
        let name = match entry.file_name().into_string() {
            Ok(name) => name,
            // A name the file system holds that is not valid UTF-8 is not a
            // unit name the parser accepts either.
            Err(_) => continue,
        };
        let text =
            std::fs::read(&path).map_err(|error| CommandError::new("io", format!("{}: {error}", path.display())))?;
        let lines = String::from_utf8_lossy(&text).lines().count();
        scripts.push(ScriptInfo {
            name,
            lines: u32::try_from(lines).unwrap_or(u32::MAX),
        });
    }
    scripts.sort_by(|a, b| ansi_compare_text(&a.name, &b.name));
    Ok(scripts)
}

fn script_list(_session: &mut Session, request: ScriptListRequest) -> Result<ScriptListResponse, CommandError> {
    let folder = scripts_folder(request.scripts.as_deref());
    let scripts = list_scripts(&folder)?;
    Ok(ScriptListResponse {
        folder: folder.display().to_string(),
        scripts,
    })
}

/// `script.check`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptCheckRequest {
    /// Paths of the scripts to parse.
    pub files: Vec<String>,
    /// Folder the units a script `uses` resolve from. Default: the
    /// `XEDIT_SCRIPTS` environment variable, else the `Edit Scripts` folder
    /// below `XEDIT_ORACLE_DIR`.
    pub scripts: Option<String>,
}

/// One syntax failure.
#[derive(Serialize, JsonSchema)]
pub struct ScriptErrorInfo {
    /// The file the failure was raised in: the script, or a unit it uses.
    pub file: String,
    /// The line, counted as the interpreter counts it (`#13` characters
    /// before the position, plus one); 0 when the file could not be read.
    pub line: i64,
    /// The interpreter's message, as `EJvInterpreterError` formats it.
    pub message: String,
}

/// The result of one script.
#[derive(Serialize, JsonSchema)]
pub struct ScriptFileResult {
    /// The script as it was given.
    pub file: String,
    /// Whether the script and the units it uses compile.
    pub ok: bool,
    pub errors: Vec<ScriptErrorInfo>,
}

/// `script.check`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ScriptCheckResponse {
    /// Whether every script compiles.
    pub ok: bool,
    /// The scripts folder the units resolved from.
    pub scripts_folder: String,
    /// The scripts checked.
    pub total: usize,
    /// The scripts that compile.
    pub parsed: usize,
    /// The scripts that do not compile.
    pub failed: usize,
    pub files: Vec<ScriptFileResult>,
}

fn script_check(_session: &mut Session, request: ScriptCheckRequest) -> Result<ScriptCheckResponse, CommandError> {
    if request.files.is_empty() {
        return Err(CommandError::new("invalid_params", "no files given"));
    }
    let folder = scripts_folder(request.scripts.as_deref());
    if !folder.is_dir() {
        return Err(CommandError::new("io", format!("{}: not a folder", folder.display())));
    }
    let mut files = Vec::new();
    let mut parsed = 0usize;
    for file in &request.files {
        let check = xedit_script::check::check_file(Path::new(file), &folder);
        let ok = check.ok();
        if ok {
            parsed += 1;
        }
        files.push(ScriptFileResult {
            file: check.file,
            ok,
            errors: check
                .errors
                .into_iter()
                .map(|error| ScriptErrorInfo {
                    file: error.file,
                    line: error.line,
                    message: error.message,
                })
                .collect(),
        });
    }
    let total = files.len();
    Ok(ScriptCheckResponse {
        ok: parsed == total,
        scripts_folder: folder.to_string_lossy().into_owned(),
        total,
        parsed,
        failed: total - parsed,
        files,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "script.list",
        "List the *.pas scripts of the scripts folder (XEDIT_SCRIPTS, the oracle's Edit Scripts by default).",
        false,
        script_list,
    );
    registry.register(
        "script.check",
        "Compile scripts and the units they use, as TJvInterpreterUnit.Compile does (JvInterpreterParser/JvInterpreter), resolved from the scripts folder; reports each failure as file:line: message.",
        false,
        script_check,
    );
    registry.register(
        "script.run",
        "Run a Pascal script over the loaded plugins as xEdit's -script: mode does (DoRunScript/ApplyScript): Initialize, Process per selected element, Finalize, then save the plugins the script changed. Needs --edit unless --dry-run.",
        true,
        script_run,
    );
}

//=== script.run: the run loop ================================================

/// `script.run`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptRunRequest {
    /// The script to run: a path, or a name below the scripts folder. A
    /// relative path resolves against the scripts folder, as `-script:`
    /// resolves against `wbScriptsPath` (`xeMainForm.pas:5060`).
    pub file: String,
    /// The scripts folder (`-S:`): the `XEDIT_SCRIPTS` environment variable,
    /// else the oracle's `Edit Scripts`. A `-script:` run reads the units a
    /// script uses from the script's own folder instead, whatever this names.
    pub scripts: Option<String>,
    /// Save every plugin the script changed at the end, as the GUI does when
    /// it closes (`SaveChanged` at the end of an auto mode, silently; an
    /// existing file is moved to the backup folder, the dialog's default).
    /// The default; `false` leaves the changes in memory for `files.save`.
    #[serde(default = "default_true")]
    pub save: bool,
    /// Run the script with editing disabled and save nothing: the script's
    /// own output is produced as it is, and a script that changes a plugin
    /// fails where it would without `--edit`.
    #[serde(default)]
    pub dry_run: bool,
    /// Build the reference information before the script runs, as the GUI's
    /// script mode does at its load (`wbBuildRefs`, off with xEdit's
    /// `-nobuildrefs`): the scripts that read `ReferencedBy` need it. Off by
    /// default; the other commands build it on demand.
    #[serde(default)]
    pub build_refs: bool,
}

fn default_true() -> bool {
    true
}

/// One loaded file of a run.
pub use crate::tool_modes::ToolModeFile as ScriptRunFile;

/// `script.run`: the response.
#[derive(Serialize, JsonSchema)]
pub struct ScriptRunResponse {
    /// The script file that ran.
    pub file: String,
    /// The unit name of the run: the file name without its extension, as
    /// `ChangeFileExt(ExtractFileName(xeScriptToRun), '')` gives it.
    pub script: String,
    /// Editing was disabled and nothing was saved.
    pub dry_run: bool,
    /// The elements `Process` ran on.
    pub processed: u64,
    /// The script raised: the message log holds the exception and the
    /// `Aborted: Applying script` line. Plugins the script changed before
    /// the failure are still saved at the end, unless `dry_run` is set or
    /// saving is off (the GUI's close saves them too).
    pub aborted: bool,
    /// The `Initialize`/`Process`/`Finalize` result that ended the run
    /// (`Script terminated itself, Result=<n>`), if one did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_terminated: Option<i64>,
    /// The failure's message, as the interpreter reports it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// `unit <name> line <n>` of the failure
    /// (`GetLastErrorLocation`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_location: Option<String>,
    /// Per plugin: whether the script changed it and whether it was saved.
    pub files: Vec<ScriptRunFile>,
    /// The lines of xEdit's message log (`Start:`/`Done:`, the script's own
    /// messages, and the failure lines).
    pub messages: Vec<String>,
}

/// The elements the run loop walks: the port of `SelectRootNodes(vstNav)` of
/// `DoRunScript` and the per-root walk of `ApplyScriptToSelection`.
///
/// The default is [`NavigationSelection`], the port of the GUI's navigation
/// tree walk. The adapter steps (phase 6 steps 5 to 9) bind the element API
/// against this: an element is the [`ElementRef`] of
/// `xedit_core::interface`, a script value is
/// [`element_value`](ScriptRunState::element_value), and the filter of
/// [`ScriptRunState::can_process`] reads the element's type and signature.
pub trait ScriptSelection {
    /// The loaded plugins the script sees, in load order (`frmMain.Files`).
    fn files(&mut self, session: &mut Session) -> Result<Vec<ElementRef>, CommandError>;

    /// The elements `Process` runs over, in the order the loop walks them.
    ///
    /// `DoRunScript` selects every root node of the navigation tree (all
    /// loaded files) and `ApplyScriptToSelection` walks each selected node's
    /// subtree *backwards*: it starts at `vstNav.GetLast(node)` and steps
    /// `GetPrevious` until the node itself, so the last element of a file is
    /// processed first and the file node last. [`NavigationSelection`] gives
    /// that order.
    fn selection(&mut self, session: &mut Session, files: &[ElementRef]) -> Result<Vec<ElementRef>, CommandError>;
}

/// The port of the navigation tree walk: every loaded file, each of the
/// file's elements in the reverse of the tree's flat order.
///
/// The tree holds the file, its groups and its records (`vstNavInitChildren`
/// gives every container element a node) and, for a record, the child group
/// that holds the record's children (`vstNavInitNode`: a main record's node
/// container is `ChildGroup`, not the record's own elements), so the walk
/// reaches records through their groups. The port's walk gives the same set:
/// the default filter of a run is `[etMainRecord]`
/// (`ApplyScript`), so with no script variable set it is every main record of
/// the loaded files, each file's records in reverse FormID order (the
/// file's `flRecords` order).
#[derive(Default)]
pub struct NavigationSelection;

impl ScriptSelection for NavigationSelection {
    fn files(&mut self, session: &mut Session) -> Result<Vec<ElementRef>, CommandError> {
        let files = crate::conflicts::session_files(session)?;
        Ok(files.iter().filter_map(|file| file.self_element_ref()).collect())
    }

    fn selection(&mut self, _session: &mut Session, files: &[ElementRef]) -> Result<Vec<ElementRef>, CommandError> {
        let mut order = Vec::new();
        for file in files {
            let mut flat = Vec::new();
            flatten(file, &mut flat);
            flat.reverse();
            order.extend(flat);
        }
        Ok(order)
    }
}

/// The flat (pre-order) nav-tree sequence of `root`, as
/// `vstNavInitChildren`/`vstNavInitNode` build the GUI's tree: a node per
/// container element, and for a main record the child group that *follows*
/// it (`NodeData.Container := GroupRecord`, `xeMainForm.pas:20228-20237`;
/// the `ChildGroup` fallback of `:20249` runs only on a node re-init, so a
/// record whose group is not its next sibling has no children here). A
/// parented group whose label is the FormID of the record before it is that
/// record's hidden node (`:20299-20347`): it contributes no element, and
/// its records are reached through the record -- exactly once.
///
/// A main record's node children are that group *alone* (`:20285-20288`
/// assigns `NodeData.Container := GroupRecord` for an `etMainRecord`); its
/// own element container holds no tree nodes, so it is never descended --
/// `MainRecordImpl::as_container` answers with it, and descending it fired
/// `Process` for subrecord-level elements the GUI tree does not hold (the
/// review probe: `ScriptProcessElements := [etSubRecord]` over SSE
/// `Update.esm` processed 4,373,854 elements where the oracle processes
/// none).
fn flatten(root: &ElementRef, out: &mut Vec<ElementRef>) {
    out.push(root.clone());
    let Some(container) = root.as_container() else {
        return;
    };
    let count = container.get_element_count();
    // Set while the loop stands on the group the record before consumed.
    let mut skip_next = false;
    for index in 0..count {
        let Some(child) = container.get_element(index) else {
            skip_next = false;
            continue;
        };
        if skip_next {
            skip_next = false;
            continue;
        }
        if child.get_element_type() == ElementType::etMainRecord {
            out.push(child.clone());
            if let Some(next) = container.get_element(index + 1)
                && let Some(group) = next.as_element_impl().and_then(ElementImpl::group_record_impl)
                && is_parented_group(group.group_type())
                && let Some(record) = child.as_element_impl().and_then(ElementImpl::main_record_impl)
                && group.get_group_label() == record.mr_struct().form_id.to_cardinal()
            {
                flatten(&next, out);
                skip_next = true;
            }
            continue;
        }
        flatten(&child, out);
    }
}

/// `ParentedGroupRecordType` (`xeMainForm.pas:5157`): world, cell and topic
/// children, quest children with `wbVWDAsQuestChildren`.
fn is_parented_group(group_type: i32) -> bool {
    matches!(group_type, 1 | 6 | 7) || (group_type == 10 && xedit_core::interface::globals::vwd_as_quest_children())
}

/// An element as the script sees it: the value of the `IInterface` parameter
/// a script's `Process(e: IInterface)` takes, and of the file functions
/// (`FileByIndex`, `MasterByIndex`).
///
/// The element API of phase 6 steps 5 to 9 downcasts it with
/// [`element_of`](ScriptElement::element_of) or
/// [`ScriptRunState::element_of`], so an adapter function reads
/// `Args.Values[0]` as the [`ElementRef`] it works on. Upstream the value is
/// `varUnknown` carrying the `IwbElement` interface; the port carries it as
/// `varObject`, the object value model of `crates/xedit-script`, whose class
/// is the `IwbElement` the run registers (steps 5 to 9 extend the class
/// chain with `IwbMainRecord`, `IwbFile` and the rest for the `is` checks of
/// scripts).
pub struct ScriptElement {
    element: ElementRef,
    class: Rc<ClassDef>,
}

impl ScriptElement {
    /// The element the value carries, `None` for a value that is not one.
    pub fn element_of(value: &Value) -> Option<ElementRef> {
        let Value::Object(object) = value else {
            return None;
        };
        let borrowed = object.borrow();
        let wrapper = borrowed.as_any().downcast_ref::<ScriptElement>()?;
        Some(wrapper.element.clone())
    }
}

impl ScriptObject for ScriptElement {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn is_interface(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The script variable `OnSetValue` writes (`xejviScriptHost.pas:60`): the
/// fields of `TfrmMain` a script sets before `ApplyFilter`, and the two the
/// run loop itself reads.
#[derive(Clone, Debug)]
pub struct ScriptVariables {
    /// `ScriptProcessElements`: the element types `Process` runs on; reset to
    /// `[etMainRecord]` before every script and set to `[etMainRecord]` again
    /// when a script empties the set.
    pub process_elements: ElementTypes,
    /// `ScriptProcessSignatures`: the upper-cased comma separated list of
    /// signatures `ScriptCanProcessElement` searches as a substring; empty
    /// takes every signature.
    pub process_signatures: String,
    /// `FilterScripted` (`cbScriptedFilter`): the port has no scripted filter
    /// function yet; the value is recorded for the GUI phase.
    pub scripted: bool,
    pub conflict_all: bool,
    pub conflict_all_set: ConflictAllSet,
    pub conflict_this: bool,
    pub conflict_this_set: ConflictThisSet,
    pub by_inject_status: bool,
    pub inject_status: bool,
    pub by_not_reachable_status: bool,
    pub not_reachable_status: bool,
    pub by_references_injected_status: bool,
    pub references_injected_status: bool,
    pub by_editor_id: bool,
    pub editor_id: String,
    pub by_element_value: bool,
    pub element_value: String,
    pub by_regex_comparison: bool,
    pub by_name: bool,
    pub name: String,
    pub by_base_editor_id: bool,
    pub base_editor_id: String,
    pub by_base_name: bool,
    pub base_name: String,
    pub scaled_actors: bool,
    pub by_signature: bool,
    pub signatures: String,
    pub by_base_signature: bool,
    pub base_signatures: String,
    pub by_persistent: bool,
    pub persistent: bool,
    pub unnecessary_persistent: bool,
    pub master_is_temporary: bool,
    pub is_master: bool,
    pub persistent_pos_changed: bool,
    pub deleted: bool,
    pub by_vwd: bool,
    pub vwd: bool,
    pub by_has_vwd_mesh: bool,
    pub has_vwd_mesh: bool,
    pub by_has_precombined_mesh: bool,
    pub has_precombined_mesh: bool,
    pub flatten_blocks: bool,
    pub flatten_cell_childs: bool,
    pub assign_pers_wrld_child: bool,
    pub inherit_conflict_by_parent: bool,
}

/// `TConflictAllSet`: the set the `clbConflictAll` checkbox list fills
/// (upstream a `set of TConflictAll`).
pub type ConflictAllSet = ElementLikeSet<ConflictAll>;
/// `TConflictThisSet`.
pub type ConflictThisSet = ElementLikeSet<ConflictThis>;

/// A Pascal `set of` a conflict enumeration, as the port's [`EnumSet`] is a
/// set of an element enumeration. The two upstream conflict enums are plain
/// `pascal_enum!`s, so one wrapper serves both.
pub type ElementLikeSet<T> = xedit_core::interface::EnumSet<T>;

impl Default for ScriptVariables {
    fn default() -> Self {
        ScriptVariables {
            process_elements: ElementTypes::of(&[ElementType::etMainRecord]),
            process_signatures: String::new(),
            scripted: false,
            conflict_all: false,
            conflict_all_set: ElementLikeSet::empty(),
            conflict_this: false,
            conflict_this_set: ElementLikeSet::empty(),
            by_inject_status: false,
            inject_status: false,
            by_not_reachable_status: false,
            not_reachable_status: false,
            by_references_injected_status: false,
            references_injected_status: false,
            by_editor_id: false,
            editor_id: String::new(),
            by_element_value: false,
            element_value: String::new(),
            by_regex_comparison: false,
            by_name: false,
            name: String::new(),
            by_base_editor_id: false,
            base_editor_id: String::new(),
            by_base_name: false,
            base_name: String::new(),
            scaled_actors: false,
            by_signature: false,
            signatures: String::new(),
            by_base_signature: false,
            base_signatures: String::new(),
            by_persistent: false,
            persistent: false,
            unnecessary_persistent: false,
            master_is_temporary: false,
            is_master: false,
            persistent_pos_changed: false,
            deleted: false,
            by_vwd: false,
            vwd: false,
            by_has_vwd_mesh: false,
            has_vwd_mesh: false,
            by_has_precombined_mesh: false,
            has_precombined_mesh: false,
            flatten_blocks: false,
            flatten_cell_childs: false,
            assign_pers_wrld_child: false,
            inherit_conflict_by_parent: false,
        }
    }
}

impl ScriptVariables {
    /// The filter options the recorded variables give, as
    /// `mniNavFilterApplyClick` builds them from the `TfrmMain` fields when
    /// `FilterPreset` skips the dialog.
    pub fn filter_options(&self) -> crate::filter::FilterRequestOptions {
        use crate::conflicts::{ConflictAllName, ConflictThisName};
        crate::filter::FilterRequestOptions {
            conflict_all: self.conflict_all.then(|| {
                values_of(self.conflict_all_set)
                    .into_iter()
                    .map(ConflictAllName::from)
                    .collect()
            }),
            conflict_this: self.conflict_this.then(|| {
                values_of(self.conflict_this_set)
                    .into_iter()
                    .map(ConflictThisName::from)
                    .collect()
            }),
            by_inject_status: self.by_inject_status.then_some(self.inject_status),
            by_not_reachable_status: self.by_not_reachable_status.then_some(self.not_reachable_status),
            by_references_injected_status: self
                .by_references_injected_status
                .then_some(self.references_injected_status),
            by_editor_id: self.by_editor_id.then(|| self.editor_id.clone()),
            by_element_value: self.by_element_value.then(|| self.element_value.clone()),
            by_name: self.by_name.then(|| self.name.clone()),
            by_base_editor_id: self.by_base_editor_id.then(|| self.base_editor_id.clone()),
            by_base_name: self.by_base_name.then(|| self.base_name.clone()),
            scaled_actors: self.scaled_actors,
            by_signature: self.by_signature.then(|| self.signatures.clone()),
            by_base_signature: self.by_base_signature.then(|| self.base_signatures.clone()),
            by_persistent: self.by_persistent,
            persistent: self.persistent,
            unnecessary_persistent: self.unnecessary_persistent,
            master_is_temporary: self.master_is_temporary,
            is_master: self.is_master,
            persistent_pos_changed: self.persistent_pos_changed,
            deleted: self.deleted,
            by_vwd: self.by_vwd.then_some(self.vwd),
            by_has_vwd_mesh: self.by_has_vwd_mesh.then_some(self.has_vwd_mesh),
            by_has_precombined_mesh: self.by_has_precombined_mesh.then_some(self.has_precombined_mesh),
            regex_comparison: self.by_regex_comparison,
            flatten_blocks: self.flatten_blocks,
            flatten_cell_childs: self.flatten_cell_childs,
            assign_pers_wrld_child: self.assign_pers_wrld_child,
            inherit_conflict_by_parent: self.inherit_conflict_by_parent,
        }
    }
}

/// The values of a Pascal set, in declaration order.
fn values_of<T: PascalEnum>(set: ElementLikeSet<T>) -> Vec<T> {
    T::ALL.iter().copied().filter(|value| set.contains(*value)).collect()
}

/// The script list objects the adapter steps append to: `TList` and
/// `TStrings` of the JVCL `Classes` unit (phase 6 step 8), which the DF
/// adapter's `GetAssetsList` and `...ByType` methods fill with the object a
/// script passes in. The unit that registers the classes sets the two
/// closures; until then the calls that need one raise a clear error.
/// `list_add` appends an item to a `TList`, `strings_add` a string to a
/// `TStrings`.
#[derive(Clone, Default)]
pub struct ScriptLists {
    pub list_add: Option<ScriptListAdd>,
    pub strings_add: Option<ScriptStringsAdd>,
}

/// `TList(V2O(list)).Add(value)`: the list object and the item appended.
pub type ScriptListAdd = Rc<dyn Fn(&Value, Value) -> Result<(), ValueError>>;
/// `TStrings(V2O(list)).Add(text)`: the list object and the text appended.
pub type ScriptStringsAdd = Rc<dyn Fn(&Value, &str) -> Result<(), ValueError>>;

impl ScriptLists {
    /// Whether any list implementation is registered.
    pub fn is_empty(&self) -> bool {
        self.list_add.is_none() && self.strings_add.is_none()
    }
}

/// The state of one `script.run`: the script variables `OnSetValue` writes,
/// the message log the run produces, the loaded files the file functions
/// read, and the counters of the progress callback.
pub struct ScriptRunState {
    /// `frmMain`'s filter fields, which a script writes and `ApplyFilter`
    /// reads.
    pub variables: ScriptVariables,
    /// The lines of xEdit's message log (`mmoMessages`).
    pub messages: Vec<String>,
    /// `frmMain.Files`: the loaded plugins, in load order.
    files: Vec<ElementRef>,
    /// The class of the elements a script receives (`IwbElement`).
    element_class: Rc<ClassDef>,
    /// `_ProgressLockCount` (`wbProgressLock`/`wbProgressUnlock`): while it is
    /// positive the messages that go through `wbProgress` are dropped --
    /// a `Silent` script's, and the walk's while `Process` runs.
    progress_lock: i32,
    /// `wbHideStartTime`: `Inc`ed around every call into the script, which
    /// strips the `[mm:ss]` prefix from the messages of that call.
    hide_start_time: i32,
    /// `wbShowStartTime`: `PerformLongAction` increments it, so the lines of
    /// the action carry the time.
    show_start_time: i32,
    /// `frmMain.FilterApplied`: whether a filter is on the tree.
    pub filter_applied: bool,
    /// The filter call the script asked for; the run loop runs it with the
    /// session when the call that asked returns (see [`PendingFilter`]). A
    /// second call replaces the first: what a filter leaves on the tree is
    /// its last state.
    pub pending_filter: Option<PendingFilter>,
    /// When the run began, for `wbFormatElapsedTime`.
    started: std::time::Instant,
    /// The script list objects the adapter steps append to (`TList`,
    /// `TStrings`); none until the JVCL `Classes` unit registers them.
    pub lists: ScriptLists,
    /// The relay of the process-wide progress callback: everything a run
    /// logs belongs in the one memo (`mmoMessages`), and that callback
    /// cannot hold this state, so [`run_script`] installs a relay whose
    /// lines [`ScriptRunState::add_message`] drains in order.
    relayed: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

/// One `ApplyFilter`/`RemoveFilter` a script called. The adapter function
/// cannot hold the session, so it leaves the request here and the run loop
/// applies it as soon as the call into the script returns.
pub enum PendingFilter {
    /// The request is boxed: it carries the whole filter dialog
    /// (`FilterRequestOptions`), and `Remove` has no data.
    Apply(Box<crate::filter::FilterApplyRequest>),
    Remove,
}

impl ScriptRunState {
    pub fn new(files: Vec<ElementRef>, element_class: Rc<ClassDef>) -> Self {
        ScriptRunState {
            variables: ScriptVariables::default(),
            messages: Vec::new(),
            files,
            element_class,
            progress_lock: 0,
            hide_start_time: 0,
            show_start_time: 0,
            filter_applied: false,
            pending_filter: None,
            started: std::time::Instant::now(),
            lists: ScriptLists::default(),
            relayed: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// The script value of an element: the `IInterface` value the element API
    /// functions take (see [`ScriptElement`]).
    pub fn element_value(&self, element: &ElementRef) -> Value {
        Value::Object(Rc::new(RefCell::new(ScriptElement {
            element: element.clone(),
            class: self.element_class.clone(),
        })))
    }

    /// The element a script value carries, for the adapter functions of
    /// steps 5 to 9.
    pub fn element_of(value: &Value) -> Option<ElementRef> {
        ScriptElement::element_of(value)
    }

    /// `frmMain.AddMessage(const s: string)`: the parts of a message of
    /// several `CRLF` lines, each its own line of the log, straight to the
    /// log (no `wbProgress`, so no lock and no time).
    pub fn add_message(&mut self, text: &str) {
        // The lines the progress relay holds first: they were posted before
        // this one reached the memo (`PostAddMessage`'s order).
        for line in self.relayed.lock().unwrap().drain(..) {
            for part in line.split("\r\n") {
                self.messages.push(part.to_owned());
            }
        }
        if text.is_empty() {
            return;
        }
        // The GUI's memo holds its text in the ANSI code page: a character
        // outside it prints as `?` (`AddMessage('Ж')` logs `?`, probe
        // `u-09`).
        let text = xedit_io::encoding::ansi_string(&xedit_io::encoding::ansi_bytes(text));
        for line in text.split("\r\n") {
            self.messages.push(line.to_owned());
        }
    }

    /// `wbProgress(aStatus)` of the GUI callback
    /// (`GeneralProgressNoAbortCheck`): dropped while the lock is on (unless
    /// `force`, the `wbProgress(..., True)` of the exception line), prefixed
    /// with `[mm:ss]` while `wbShowStartTime > 0` and inside no script call.
    pub fn progress(&mut self, text: &str, force: bool) {
        if text.is_empty() || (self.progress_lock > 0 && !force) {
            return;
        }
        // `PostAddMessage` hands the text to `AddMessage`, which splits it
        // into the lines of the log; the prefix is on the whole text.
        let line = if self.show_start_time > 0 && self.hide_start_time < 1 {
            let elapsed = elapsed_text(self.started.elapsed());
            format!("[{elapsed}] {text}")
        } else {
            text.to_owned()
        };
        self.add_message(&line);
    }

    /// `ScriptCanProcessElement` (`xeMainForm.pas:8654`): the element's type
    /// is in `ScriptProcessElements`, and a main record's signature is a
    /// substring of `ScriptProcessSignatures` (`ContainsStr`, not a list
    /// search).
    pub fn can_process(&self, element: &ElementRef) -> bool {
        if !self.variables.process_elements.contains(element.get_element_type()) {
            return false;
        }
        if element.get_element_type() == ElementType::etMainRecord && !self.variables.process_signatures.is_empty() {
            let signature = element.get_has_signature().map(signature_text).unwrap_or_default();
            if !self.variables.process_signatures.contains(&signature) {
                return false;
            }
        }
        true
    }

    /// `Length(frmMain.Files)`.
    pub(crate) fn file_count(&self) -> usize {
        self.files.len()
    }

    pub(crate) fn file_at(&self, index: i64) -> Option<ElementRef> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.files.get(index).cloned())
    }

    /// `_File.MasterCount[True]` (`IwbFile_MasterCount`).
    ///
    /// The hardcoded file's records resolve into the game master, so upstream
    /// counts the game master as its single master (`MasterByIndex(exe, 0)`
    /// answers it; `List loaded plugins and their masters.pas` reads both for
    /// the executable).
    pub(crate) fn master_count(&self, file: &ElementRef) -> Option<i32> {
        let file = file.get_file()?;
        if file.get_file_states().contains(FileState::fsIsHardcoded) {
            return Some(i32::from(game_master_element().is_some()));
        }
        Some(file.get_master_count(true))
    }

    /// `_File.Masters[j, True]` (`IwbFile_MasterByIndex`).
    pub(crate) fn master_at(&self, file: &ElementRef, index: i64) -> Option<ElementRef> {
        let file = file.get_file()?;
        if file.get_file_states().contains(FileState::fsIsHardcoded) {
            return if index == 0 { game_master_element() } else { None };
        }
        let index = i32::try_from(index).ok()?;
        if index >= file.get_master_count(true) {
            return None;
        }
        file.get_master(index, true)
            .and_then(|master| master.as_element_impl().and_then(ElementImpl::self_element_ref))
    }
}

/// `TwbSignature` as a string: four characters.
pub(crate) fn signature_text(signature: Signature) -> String {
    String::from_utf8_lossy(&signature.0).into_owned()
}

/// `wbFormatElapsedTime` (`xeMainForm.pas:1361`): `FormatDateTime('nn:ss')`,
/// the hours in front once there are any.
fn elapsed_text(elapsed: std::time::Duration) -> String {
    let seconds = elapsed.as_secs();
    let hours = seconds / 3600;
    let text = format!("{:02}:{:02}", (seconds / 60) % 60, seconds % 60);
    if hours > 0 { format!("{hours}:{text}") } else { text }
}

/// The loaded game master's element, the master the hardcoded file's
/// records resolve into.
pub(crate) fn game_master_element() -> Option<ElementRef> {
    xedit_core::implementation::game_master_file()
        .and_then(|file| file.as_element_impl().and_then(ElementImpl::self_element_ref))
}

/// `TwbFile.GetName` (`wbImplementation.pas:4853-4859`): the load order
/// prefix (`[00] `) in front of the file name, the executable's name for the
/// hardcoded file (`[00] SkyrimSE.exe`); every other element keeps its own
/// `Name`.
fn element_name(element: &ElementRef) -> String {
    if element.get_element_type() != ElementType::etFile {
        return element.get_name();
    }
    let Some(file) = element.get_file() else {
        return element.get_name();
    };
    let name = if file.get_file_states().contains(FileState::fsIsHardcoded) {
        xedit_core::interface::globals::game_exe_name()
    } else {
        file.get_name()
    };
    format!("[{}] {}", file.get_load_order_file_id(), name)
}

/// The unit the adapter units register the shared functions under (`cUnit`
/// of `xejviScriptAdapter*.pas`, `'Dummy'` upstream): a name with no file,
/// which `uses` never resolves, so the functions are callable from any
/// script.
pub(crate) const C_UNIT: &str = "";

/// Registers the session side of the script API: the `xejviScriptAdapterHost.pas`
/// functions that need no form (the `TfrmMain` fields they read are the
/// [`ScriptRunState`]), the file functions of `xejviScriptAdapterFile.pas`
/// and the `Name` of `xejviScriptAdapterElement.pas` a script over the loaded
/// plugins needs, the `et*`/`ca*`/`ct*` constants of
/// `xejviScriptAdapter.pas` the filter hooks read, and the whole data format
/// surface of `xejviScriptAdapterDF.pas` (`crate::script_df::register`,
/// phase 6 step 6).
///
/// The remaining registrations of those units (the record, element and
/// container API, `RecordByFormID`, `AddNewFile`, the conflict helpers, the
/// LOD and atlas helpers, and everything of the form) belong to phase 6
/// steps 5, 7, 8 and 9, which extend this function.
///
/// The element values the functions hand out are [`ScriptElement`]s; the
/// element API of the later steps downcasts them with
/// [`ScriptRunState::element_of`].
///
/// `vcl` is the VCL seam (the form shim of phase 6 step 9): the four
/// `JvInterpreterFm.pas` registrations, the members of the ten VCL-backed
/// JVCL units and `xejviScriptAdapterMiscVcl.pas` route through it. Headless,
/// [`crate::script_rtl::NoVcl`] is passed.
fn register_functions(
    adapter: &mut Adapter,
    state: &Rc<RefCell<ScriptRunState>>,
    vcl: Rc<RefCell<dyn crate::script_rtl::Vcl>>,
) {
    fn host_function(
        adapter: &mut Adapter,
        identifier: &str,
        param_count: i32,
        param_types: Vec<ParamType>,
        func: xedit_script::adapter::AdapterGetFn,
    ) {
        adapter.add_function(C_UNIT, identifier, func, param_count, param_types, VarType::Empty);
    }
    let value_param = || vec![ParamType::value(VarType::Empty)];
    let integer_param = || vec![ParamType::value(VarType::Integer)];
    let no_params: Vec<ParamType> = Vec::new();

    //=== The game and the paths (xejviScriptAdapterHost.pas) ================

    host_function(
        adapter,
        "wbGameMode",
        0,
        no_params.clone(),
        Rc::new(|value: &mut Value, _args: &mut Args| {
            *value = Value::Integer(xedit_core::interface::globals::game_mode() as i32);
            Ok(())
        }),
    );
    let string_global = |adapter: &mut Adapter, identifier: &str, getter: fn() -> String| {
        host_function(
            adapter,
            identifier,
            0,
            Vec::new(),
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                *value = Value::Str(getter().into_bytes());
                Ok(())
            }),
        );
    };
    use xedit_core::interface::globals;
    string_global(adapter, "wbGameName", globals::game_name);
    string_global(adapter, "wbGameMasterEsm", globals::game_master_esm);
    string_global(adapter, "wbGameName2", globals::game_name2);
    string_global(adapter, "wbAppName", globals::app_name);
    string_global(adapter, "ProgramPath", globals::program_path);
    string_global(adapter, "wbProgramPath", globals::program_path);
    string_global(adapter, "ScriptsPath", globals::scripts_path);
    string_global(adapter, "wbScriptsPath", globals::scripts_path);
    string_global(adapter, "DataPath", globals::data_path);
    string_global(adapter, "wbDataPath", globals::data_path);
    string_global(adapter, "TempPath", globals::temp_path);
    string_global(adapter, "wbTempPath", globals::temp_path);
    string_global(adapter, "wbOutputPath", globals::output_path);
    host_function(
        adapter,
        "dfFloatDecimalDigits",
        0,
        no_params.clone(),
        Rc::new(|value: &mut Value, _args: &mut Args| {
            let digits = xedit_assets::data_format::FLOAT_DECIMAL_DIGITS.load(std::sync::atomic::Ordering::Relaxed);
            *value = Value::Integer(digits as i32);
            Ok(())
        }),
    );

    //=== The message log (xejviScriptAdapterHost.pas) ========================

    {
        let state = state.clone();
        host_function(
            adapter,
            "AddMessage",
            1,
            value_param(),
            Rc::new(move |_value: &mut Value, args: &mut Args| {
                // `if VarIsStr(Args.Values[0]) then wbProgress(...) else
                // JvInterpreterError(ieDirectInvalidArgument, 0)`.
                let Value::Str(text) = &args.values[0] else {
                    return Err(ValueError::interp(IE_DIRECT_INVALID_ARGUMENT));
                };
                let text = String::from_utf8_lossy(text).into_owned();
                state.borrow_mut().progress(&text, false);
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        host_function(
            adapter,
            "ClearMessages",
            0,
            no_params.clone(),
            Rc::new(move |_value: &mut Value, _args: &mut Args| {
                state.borrow_mut().messages.clear();
                Ok(())
            }),
        );
    }

    //=== The loaded files (xejviScriptAdapterHost.pas, the `IwbFile` slice
    // of xejviScriptAdapterFile.pas and `Name` of
    // xejviScriptAdapterElement.pas) =========================================

    {
        let state = state.clone();
        host_function(
            adapter,
            "FileCount",
            0,
            no_params.clone(),
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                // `Value := Length(frmMain.Files)`
                // (`xejviScriptAdapterHost.pas:166`): `Length` of a dynamic
                // array is a `NativeInt`, so the variant is a `varInt64`
                // (`VarType(FileCount)` is 20 on the oracle, probe
                // `v-slot-35`), which `Format('%d', [FileCount])` refuses.
                *value = Value::Int64(state.borrow().file_count() as i64);
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        host_function(
            adapter,
            "FileByIndex",
            1,
            value_param(),
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let index = xedit_script::values::to_i32(&args.values[0])
                    .map_err(|_| ValueError::interp(IE_DIRECT_INVALID_ARGUMENT))?;
                let state = state.borrow();
                let Some(file) = state.file_at(i64::from(index)) else {
                    return Err(ValueError::interp(IE_DIRECT_INVALID_ARGUMENT));
                };
                *value = state.element_value(&file);
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        host_function(
            adapter,
            "FileByName",
            1,
            value_param(),
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let Some(Value::Str(name)) = args.values.first() else {
                    return Ok(());
                };
                let name = String::from_utf8_lossy(name).into_owned();
                let state = state.borrow();
                for file in &state.files {
                    if element_name(file).eq_ignore_ascii_case(&name) {
                        *value = state.element_value(file);
                        break;
                    }
                }
                Ok(())
            }),
        );
    }
    host_function(
        adapter,
        "Name",
        1,
        value_param(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Str(Vec::new());
            if let Some(element) = crate::script_units::element_at(args, 0) {
                *value = Value::Str(element_name(&element).into_bytes());
            }
            Ok(())
        }),
    );
    host_function(
        adapter,
        "RemoveNode",
        1,
        value_param(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Bool(false);
            let Some(element) = ScriptRunState::element_of(&args.values[0]) else {
                return Ok(());
            };
            // `_RemoveNode` (`xejviScriptAdapterHost.pas:330`): the nav
            // node's container -- a main record's child group -- leaves
            // first, then the element itself, through the same `Remove` as
            // `elements.remove`. The GUI's active-record, undo-history and
            // tree work has no headless counterpart; the walk holds its own
            // order and never re-visits a removed element.
            if element.get_element_type() == ElementType::etMainRecord
                && let Some(group) = element
                    .as_element_impl()
                    .and_then(ElementImpl::main_record_impl)
                    .and_then(|record| record.child_group())
                && let Some(group_ref) = group.self_element_ref()
            {
                group_ref.remove();
            }
            element.remove();
            *value = Value::Bool(true);
            Ok(())
        }),
    );
    host_function(
        adapter,
        "GetFileName",
        1,
        value_param(),
        Rc::new(|value: &mut Value, args: &mut Args| {
            *value = Value::Str(Vec::new());
            let name = ScriptElement::element_of(&args.values[0])
                .and_then(|element| element.get_file())
                .map(|file| file.get_name())
                .unwrap_or_default();
            *value = Value::Str(name.into_bytes());
            Ok(())
        }),
    );
    {
        let state = state.clone();
        host_function(
            adapter,
            "MasterCount",
            1,
            value_param(),
            Rc::new(move |value: &mut Value, args: &mut Args| {
                *value = Value::Integer(0);
                if let Some(element) = ScriptElement::element_of(&args.values[0]) {
                    *value = Value::Integer(state.borrow().master_count(&element).unwrap_or(0));
                }
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        host_function(
            adapter,
            "MasterByIndex",
            2,
            {
                let mut params = value_param();
                params.extend(integer_param());
                params
            },
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let Some(element) = ScriptElement::element_of(&args.values[0]) else {
                    return Ok(());
                };
                let index = v2s(&args.values[1]).map_err(|_| ValueError::interp(IE_DIRECT_INVALID_ARGUMENT))?;
                let state = state.borrow();
                if let Some(master) = state.master_at(&element, i64::from(index)) {
                    *value = state.element_value(&master);
                }
                Ok(())
            }),
        );
    }

    //=== The filter (xejviScriptAdapterHost.pas) =============================

    {
        let state = state.clone();
        host_function(
            adapter,
            "FilterApplied",
            0,
            no_params.clone(),
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                // Upstream applies the filter inside the call; the port runs
                // the request when the call returns, so a pending one
                // already counts as applied.
                let state = state.borrow();
                let applied = match &state.pending_filter {
                    Some(PendingFilter::Apply(_)) => true,
                    Some(PendingFilter::Remove) => false,
                    None => state.filter_applied,
                };
                *value = Value::Bool(applied);
                Ok(())
            }),
        );
    }
    {
        // `_ApplyFilter`: `FilterPreset := True; mniNavFilterApplyClick(nil)`.
        // The tree commands need the session, which an adapter function does
        // not hold, so the run loop applies the request when the call into
        // the script returns.
        let state = state.clone();
        host_function(
            adapter,
            "ApplyFilter",
            0,
            no_params.clone(),
            Rc::new(move |_value: &mut Value, _args: &mut Args| {
                let options = state.borrow().variables.filter_options();
                state.borrow_mut().pending_filter =
                    Some(PendingFilter::Apply(Box::new(crate::filter::FilterApplyRequest {
                        files: Vec::new(),
                        options: Some(options),
                        preset: None,
                        mod_groups: Vec::new(),
                        all_mod_groups: false,
                        saved_mod_groups: false,
                        list_records: false,
                        conflict_only: false,
                        only_one: false,
                        no_game_master: false,
                    })));
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        host_function(
            adapter,
            "RemoveFilter",
            0,
            no_params.clone(),
            Rc::new(move |_value: &mut Value, _args: &mut Args| {
                state.borrow_mut().pending_filter = Some(PendingFilter::Remove);
                Ok(())
            }),
        );
    }

    //=== The constants the script variables read (xejviScriptAdapter.pas) ====

    for element_type in ElementType::ALL {
        adapter.add_const(
            C_UNIT,
            element_type.enum_name(),
            Value::Integer(element_type.ord() as i32),
        );
    }
    for value in ConflictAll::ALL {
        adapter.add_const(C_UNIT, value.enum_name(), Value::Integer(value.ord() as i32));
    }
    for value in ConflictThis::ALL {
        adapter.add_const(C_UNIT, value.enum_name(), Value::Integer(value.ord() as i32));
    }

    //=== The data format adapter (xejviScriptAdapterDF.pas, step 6) ==========

    crate::script_df::register(adapter, state);

    //=== The classes and the misc family (phase 6 step 7) ====================

    let classes = crate::script_classes::ScriptClasses::register(adapter);
    crate::script_strings::register_base(adapter, &classes);
    crate::script_adapter::register(adapter, &classes);
    crate::script_misc::register(adapter, &classes);
    crate::script_misc_regex::register(adapter, &classes);
    crate::script_misc_json::register(adapter, &classes);

    //=== The JVCL RTL units (phase 6 step 8) ================================
    // After step 7's classes, whose `TStrings`/`TStringList` this step
    // registers the rest of (the members both units carry are registered
    // once, by the unit that comes first).

    crate::script_rtl::register(adapter, state, vcl.clone());

    //=== The VCL surface and the form shim (phase 6 step 9) ==================
    // After the RTL units: the classes of the ten VCL-backed units attach to
    // the `Classes` unit's `TComponent`/`TStringList`.

    crate::script_vcl::register(adapter, state, &vcl);
}

/// The `OnSetValue` hook (`xejviScriptHost.pas:60`): the identifiers a script
/// writes that are not variables of its own. `Ok(true)` is upstream's
/// `Done := True`; `Ok(false)` leaves the write to the interpreter, which
/// reports the undeclared identifier. The filter fields are the port's
/// [`ScriptVariables`]; `wbOutputPath` and `dfFloatDecimalDigits` are
/// globals, as upstream.
/// The `OnSetValue` hook (`xejviScriptHost.pas:60`): the identifiers a script
/// writes that are not variables of its own. `Ok(true)` is upstream's
/// `Done := True`; `Ok(false)` leaves the write to the interpreter, which
/// reports the undeclared identifier. The filter fields are the port's
/// [`ScriptVariables`]; `wbOutputPath` and `dfFloatDecimalDigits` are
/// globals, as upstream.
fn set_variable(state: &Rc<RefCell<ScriptRunState>>, identifier: &str, value: &Value) -> Result<bool, ExecError> {
    // `FilterByName := Value`: Delphi coerces the variant to the field's
    // type; a variant that does not convert is `ieTypeMismatch` here.
    let boolean = || to_bool(value).map_err(|_| ExecError::new(IE_TYPE_MISMATCH, None, "", ""));
    let text = || to_str(value).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let name = identifier.as_bytes();
    if name.eq_ignore_ascii_case(b"ScriptProcessElements") {
        // `for i := Low(TwbElementType) to High(TwbElementType) do if (v and
        // (1 shl i)) > 0 then Include(...)`; an empty set becomes
        // `[etMainRecord]`.
        let bits = u64::from(v2s(value).unwrap_or(0) as u32);
        let mut set = ElementTypes::from_bits(bits & ((1u64 << ElementType::ALL.len()) - 1));
        if set.is_empty() {
            set = ElementTypes::of(&[ElementType::etMainRecord]);
        }
        state.borrow_mut().variables.process_elements = set;
        return Ok(true);
    }
    if name.eq_ignore_ascii_case(b"ScriptProcessSignatures") {
        let Ok(text) = text() else {
            return Ok(false);
        };
        let mut list = Vec::new();
        for entry in text.split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            if entry.chars().count() != 4 {
                let message = format!(
                    "ScriptProcessSignatures expects a comma separated list of 4 character signatures. The signature \"{entry}\" is not a valid candidate."
                );
                return Err(ExecError::new(IE_DIRECT_INVALID_ARGUMENT, Some(0), &message, ""));
            }
            list.push(entry.to_ascii_uppercase());
        }
        state.borrow_mut().variables.process_signatures = list.join(",");
        return Ok(true);
    }
    if name.eq_ignore_ascii_case(b"wbOutputPath") {
        let Ok(text) = text() else {
            return Ok(false);
        };
        globals::set_output_path(&text);
        return Ok(true);
    }
    if name.eq_ignore_ascii_case(b"dfFloatDecimalDigits") {
        // `Assert(dfFloatDecimalDigits > 0, ...)` is compiled out of the
        // release build.
        let digits = to_i64(value).unwrap_or(0);
        xedit_assets::data_format::FLOAT_DECIMAL_DIGITS.store(
            usize::try_from(digits).unwrap_or(0),
            std::sync::atomic::Ordering::Relaxed,
        );
        return Ok(true);
    }
    if name.eq_ignore_ascii_case(b"FilterConflictAllSet") {
        let bits = u64::from(v2s(value).unwrap_or(0) as u32);
        state.borrow_mut().variables.conflict_all_set = ConflictAllSet::from_bits(bits);
        return Ok(true);
    }
    if name.eq_ignore_ascii_case(b"FilterConflictThisSet") {
        let bits = u64::from(v2s(value).unwrap_or(0) as u32);
        state.borrow_mut().variables.conflict_this_set = ConflictThisSet::from_bits(bits);
        return Ok(true);
    }
    // The Boolean filter fields, in the order of the Pascal; `field` writes
    // one and names it.
    macro_rules! booleans {
        ($($identifier:literal => $field:ident,)*) => {
            $(
                if name.eq_ignore_ascii_case($identifier.as_bytes()) {
                    state.borrow_mut().variables.$field = boolean()?;
                    return Ok(true);
                }
            )*
        };
    }
    macro_rules! texts {
        ($($identifier:literal => $field:ident,)*) => {
            $(
                if name.eq_ignore_ascii_case($identifier.as_bytes()) {
                    let Ok(value) = text() else {
                        return Ok(false);
                    };
                    state.borrow_mut().variables.$field = value;
                    return Ok(true);
                }
            )*
        };
    }
    texts! {
        "FilterEditorID" => editor_id,
        "FilterName" => name,
        "FilterElementValue" => element_value,
        "FilterBaseEditorID" => base_editor_id,
        "FilterBaseName" => base_name,
        "FilterSignatures" => signatures,
        "FilterBaseSignatures" => base_signatures,
    }
    booleans! {
        "FilterScripted" => scripted,
        "FilterConflictAll" => conflict_all,
        "FilterConflictThis" => conflict_this,
        "FilterByInjectStatus" => by_inject_status,
        "FilterInjectStatus" => inject_status,
        "FilterByNotReachableStatus" => by_not_reachable_status,
        "FilterNotReachableStatus" => not_reachable_status,
        "FilterByReferencesInjectedStatus" => by_references_injected_status,
        "FilterReferencesInjectedStatus" => references_injected_status,
        "FilterByEditorID" => by_editor_id,
        "FilterByElementValue" => by_element_value,
        "FilterByRegexComparison" => by_regex_comparison,
        "FilterByName" => by_name,
        "FilterByBaseEditorID" => by_base_editor_id,
        "FilterByBaseName" => by_base_name,
        "FilterScaledActors" => scaled_actors,
        "FilterBySignature" => by_signature,
        "FilterByBaseSignature" => by_base_signature,
        "FilterByPersistent" => by_persistent,
        "FilterPersistent" => persistent,
        "FilterUnnecessaryPersistent" => unnecessary_persistent,
        "FilterMasterIsTemporary" => master_is_temporary,
        "FilterIsMaster" => is_master,
        "FilterPersistentPosChanged" => persistent_pos_changed,
        "FilterDeleted" => deleted,
        "FilterByVWD" => by_vwd,
        "FilterVWD" => vwd,
        "FilterByHasVWDMesh" => by_has_vwd_mesh,
        "FilterHasVWDMesh" => has_vwd_mesh,
        "FilterByHasPrecombinedMesh" => by_has_precombined_mesh,
        "FilterHasPrecombinedMesh" => has_precombined_mesh,
        "FlattenBlocks" => flatten_blocks,
        "FlattenCellChilds" => flatten_cell_childs,
        "AssignPersWrldChild" => assign_pers_wrld_child,
        "InheritConflictByParent" => inherit_conflict_by_parent,
    }
    Ok(false)
}

/// The `Initialize`/`Process`/`Finalize` call of the run loop: the
/// `wbHideStartTime` of `ApplyScript` wraps it, so the messages the script
/// adds carry no `[mm:ss]`.
fn call_script(
    script: &mut Script,
    state: &Rc<RefCell<ScriptRunState>>,
    name: &str,
    params: &[Value],
) -> Result<Value, ExecError> {
    state.borrow_mut().hide_start_time += 1;
    let result = script.call_function(name, params);
    state.borrow_mut().hide_start_time -= 1;
    result
}

/// `Result <> 0` of `ApplyScript`: the variant comparison, where an
/// unassigned result and `Null` are zero.
fn result_as_integer(value: &Value) -> i64 {
    match value {
        Value::Empty | Value::Null => 0,
        Value::Bool(value) => i64::from(*value),
        other => xedit_script::values::to_i64(other).unwrap_or(0),
    }
}

/// Runs the filter calls the script asked for, as the call returns
/// (`ApplyFilter`/`RemoveFilter`), and folds their log lines into the run's.
///
/// `_ApplyFilter` is `FilterPreset := True; mniNavFilterApplyClick(nil)`,
/// whose `ApplyFilter` wraps the filter in
/// `PerformLongAction('Applying Filter', '[Pass 1] Processed Records: 0',
/// ...)`: a `Start: Applying Filter` line, the filter's own lines, then
/// `Done: Applying Filter, <wbCurrentProgress>` with the counts of the two
/// passes.
fn apply_pending_filters(session: &mut Session, state: &Rc<RefCell<ScriptRunState>>) -> Result<(), CommandError> {
    loop {
        // The `borrow_mut()` of a `while let` condition would live for the
        // whole body, which borrows the state again.
        let Some(pending) = state.borrow_mut().pending_filter.take() else {
            break;
        };
        match pending {
            PendingFilter::Apply(request) => {
                state.borrow_mut().progress("Start: Applying Filter", false);
                let response = crate::filter::filter_apply(session, *request)?;
                let mut state = state.borrow_mut();
                state.filter_applied = true;
                for line in &response.messages {
                    state.add_message(line);
                }
                state.progress(
                    &format!(
                        "Done: Applying Filter, [Pass 1] Processed Records: {}, [Pass 2] Processed Records: {}, Remaining unfiltered nodes: {}",
                        response.pass1, response.pass2, response.unfiltered
                    ),
                    false,
                );
            }
            PendingFilter::Remove => {
                let _ = crate::filter::filter_remove(session, crate::filter::FilterRemoveRequest {})?;
                state.borrow_mut().filter_applied = false;
            }
        }
    }
    Ok(())
}

/// `script.run`: the dry-run gate around [`run_script`], so an early return
/// (a missing script, a compile failure, a failing filter) restores the edit
/// flag.
pub fn script_run_gated(
    session: &mut Session,
    request: ScriptRunRequest,
    selection: &mut dyn ScriptSelection,
) -> Result<ScriptRunResponse, CommandError> {
    let dry_run = request.dry_run;
    let was_allowed = session.edit_allowed();
    if dry_run {
        // No write of a plugin and no change of its data: the script runs
        // with editing disabled, as it would without `--edit`.
        session.allow_edit(false);
    }
    let outcome = run_script(session, request, selection);
    if dry_run {
        session.allow_edit(was_allowed);
    }
    outcome
}

/// Sets `wbDisplayShorterNames` for a script run and restores it on drop.
struct ShorterNames(bool);

impl ShorterNames {
    fn set() -> Self {
        let previous = xedit_core::interface::globals::display_shorter_names();
        xedit_core::interface::globals::set_display_shorter_names(true);
        ShorterNames(previous)
    }
}

impl Drop for ShorterNames {
    fn drop(&mut self) {
        xedit_core::interface::globals::set_display_shorter_names(self.0);
    }
}

/// `script.run`: the port of `ApplyScript` driven by `DoRunScript`.
///
/// Restores the process-wide progress callback
/// (`xedit_core::interface::misc`) when it drops, so a run leaves the
/// callback as it was whichever path it exits by.
struct ProgressRelay(Option<xedit_core::interface::misc::ProgressCallback>);

impl ProgressRelay {
    /// Installs `relay` as the process-wide progress callback.
    fn install(relay: impl Fn(&str) + Send + Sync + 'static) -> ProgressRelay {
        let previous = xedit_core::interface::misc::progress_callback();
        xedit_core::interface::misc::set_progress_callback(Some(std::sync::Arc::new(relay)));
        ProgressRelay(previous)
    }
}

impl Drop for ProgressRelay {
    fn drop(&mut self) {
        xedit_core::interface::misc::set_progress_callback(self.0.take());
    }
}

/// The loop: `Initialize` (a non-zero result stops the run with
/// `Script terminated itself, Result=<n>`), `Process` for every selected
/// element [`ScriptRunState::can_process`] accepts (each call's result
/// checked the same way), then `Finalize`. A failure reports
/// `Exception in unit <u> line <n>: [<class>] <message>` and
/// `Aborted: Applying script "<name>"`, and nothing is saved; a run that
/// finishes logs `Done: ...` and `You can close this application now.`
/// (`DoRunScript`'s last line, which an aborted run does not reach).
///
/// The selection is the [`ScriptSelection`] seam; `script.run` itself uses
/// [`NavigationSelection`], the GUI's walk. `Process` is skipped, without
/// calling `Initialize`/`Finalize` any less, when the routine is missing or
/// empty (`FunctionIsEmpty`).
pub fn run_script(
    session: &mut Session,
    request: ScriptRunRequest,
    selection: &mut dyn ScriptSelection,
) -> Result<ScriptRunResponse, CommandError> {
    session.mode()?;
    // The GUI's script mode builds the reference information at load
    // (`wbBuildRefs`; `-nobuildrefs` turns it off): the scripts that read
    // `ReferencedBy` get it on request, as the filter builds it. The switch
    // is also the flag a session ON: the definitions gate on it
    // (`wbQUSTEventToStr` warns only with `wbBuildRefs` on), so a run
    // without `--build-refs` behaves like the oracle's `-nobuildrefs`
    // session even when the load built the index anyway.
    globals::set_build_refs(request.build_refs);
    if request.build_refs {
        session.ensure_refs()?;
    }
    // The GUI sets `wbDisplayShorterNames := True` at startup
    // (`xeMainForm.pas:5152`), so every name a script reads -- `Name`,
    // `ShortName`, `FullPath`, the run's `Processing:`-style messages -- is
    // the short form `EditorID "Full Name" [SIG:FormID]`. Restored when the
    // run ends so the other commands keep the `xDump` long names.
    let _shorter_names = ShorterNames::set();
    // `DoRunScript`: a script that is not rooted resolves against the scripts
    // folder, and one that is not there is `Could not open script: ...`.
    let folder = scripts_folder(request.scripts.as_deref());
    let mut path = PathBuf::from(&request.file);
    if !path.is_file() {
        path = folder.join(&request.file);
    }
    if !path.is_file() {
        return Err(CommandError::new(
            "io",
            format!("Could not open script: {}", path.display()),
        ));
    }
    // `DoRunScript` loads the script with `TStringList.LoadFromFile`: a byte
    // order mark selects the encoding, else the ANSI code page.
    let text = std::fs::read(&path)
        .map(|bytes| xedit_script::host::source_text(&bytes))
        .map_err(|error| CommandError::new("io", format!("{}: {error}", path.display())))?;
    let script_folder = path.parent().unwrap_or(&folder).to_path_buf();
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let action = format!("Applying script \"{name}\"");

    // `xeInit.pas`'s paths, for the script functions that read them
    // (`wbScriptsPath` is the script's folder during a run, `DoRunScript`).
    globals::set_scripts_path(&trailing_separator(&script_folder));
    if globals::output_path().is_empty() && !globals::data_path().is_empty() {
        globals::set_output_path(&trailing_separator(Path::new(&globals::data_path())));
    }
    if globals::temp_path().is_empty() {
        let temp = std::env::temp_dir().join(format!("{}Edit", globals::app_name()));
        globals::set_temp_path(&trailing_separator(&temp));
    }

    let mut response = ScriptRunResponse {
        file: path.display().to_string(),
        script: name.clone(),
        dry_run: request.dry_run,
        processed: 0,
        aborted: false,
        self_terminated: None,
        error: None,
        error_location: None,
        files: Vec::new(),
        messages: Vec::new(),
    };

    let files = selection.files(session)?;
    let order = selection.selection(session, &files)?;

    // The interpreter of the run: the adapter with the session-side
    // registrations, the script variable hook, and `wbTick` as the statement
    // hook.
    let mut interpreter = Interpreter::new();
    let element_class = interpreter.adapter.add_class(C_UNIT, "IwbElement", None);
    let state = Rc::new(RefCell::new(ScriptRunState::new(files, element_class)));
    // The VCL seam: headless (`NoVcl`) every form, dialog and control call
    // refuses with `EVclUnavailable`, naming the surface and the script
    // (phase 6 step 9). The GUI phase passes the implementation backed by the
    // real window layer.
    let vcl: Rc<RefCell<dyn crate::script_rtl::Vcl>> =
        Rc::new(RefCell::new(crate::script_rtl::NoVcl::new(name.clone())));
    register_functions(&mut interpreter.adapter, &state, vcl);
    crate::script_units::register_units(&mut interpreter.adapter, &state);
    {
        let state = state.clone();
        interpreter.on_set_value = Some(Box::new(move |identifier, value, _args| {
            set_variable(&state, identifier, value)
        }));
    }
    interpreter.on_statement = Some(Box::new(|_pos| {
        // `JvInterpreterProgramStatement` is `wbTick`, whose
        // `GeneralProgress` aborts when the run was terminated from outside
        // (`wbForceTerminate`).
        if globals::force_terminate() {
            return Err(ExecError::new(IE_USER_BREAK, None, "", ""));
        }
        Ok(())
    }));

    let mut script = match Script::create(&name, &text, &script_folder, interpreter) {
        Ok(script) => script,
        Err(error) => {
            response.aborted = true;
            response.error = Some(error.message().to_owned());
            response.error_location = Some(format!("unit {} line {}", error.unit_name, error.line));
            return Ok(response);
        }
    };
    // The process-wide progress callback (`wbProgressCallback`) is one memo
    // upstream, the script section of this run here: the definitions reach
    // it while the run initialises elements lazily (the `<Warning: Unused
    // data in: ...>` of `Add Prefix Plus Name To EDID.pas`), so the run
    // relays it into its own messages.
    let relay = state.borrow().relayed.clone();
    let _progress_relay = ProgressRelay::install(move |text| {
        relay.lock().unwrap().push(text.to_owned());
    });

    let silent = is_silent(&text);
    {
        let mut state = state.borrow_mut();
        state.show_start_time += 1;
        // The lock of a `Mode: Silent` script is on before the first line:
        // the oracle's log of `Bookmark1.pas` holds the closing line alone
        // (parity run of 2026-10-10), neither the `Start:` nor the `Done:`
        // line, which are both dropped while it is on.
        if silent {
            state.progress_lock += 1;
        }
        state.progress(&format!("Start: {action}"), false);
    }

    let mut last_error: Option<ExecError> = None;
    let mut self_terminated: Option<i64> = None;
    // `wbCurrentProgress` of the selection walk (`xeMainForm.pas:8704-8705`):
    // the count of processed elements, which the closing line repeats while
    // it is set.
    let mut processed_progress: Option<u64> = None;
    'run: {
        if script.function_exists("Initialize") && !script.function_is_empty("Initialize") {
            match call_script(&mut script, &state, "Initialize", &[]) {
                Ok(value) => {
                    apply_pending_filters(session, &state)?;
                    let result = result_as_integer(&value);
                    if result != 0 {
                        state
                            .borrow_mut()
                            .progress(&format!("Script terminated itself, Result={result}"), false);
                        self_terminated = Some(result);
                        break 'run;
                    }
                }
                Err(error) => {
                    last_error = Some(error);
                    break 'run;
                }
            }
        }
        if script.function_exists("Process") && !script.function_is_empty("Process") {
            for element in &order {
                if !state.borrow().can_process(element) {
                    continue;
                }
                let params = {
                    let state = state.borrow();
                    [state.element_value(element)]
                };
                // `ScriptProcessSingleElement`: without messages the walk
                // unlocks the progress callback around the call.
                if silent {
                    state.borrow_mut().progress_lock -= 1;
                }
                let result = call_script(&mut script, &state, "Process", &params);
                if silent {
                    state.borrow_mut().progress_lock += 1;
                }
                match result {
                    Ok(value) => {
                        apply_pending_filters(session, &state)?;
                        let result = result_as_integer(&value);
                        if result != 0 {
                            // `ScriptProcessSingleElement <> 0` exits the
                            // selection walk only (`ApplyScriptToSelection`,
                            // `xeMainForm.pas:8762`): `Finalize` still runs,
                            // and the count does not include this element
                            // (probes `t-17`, `t-29`).
                            state
                                .borrow_mut()
                                .progress(&format!("Script terminated itself, Result={result}").to_owned(), false);
                            self_terminated = Some(result);
                            break;
                        }
                    }
                    Err(error) => {
                        last_error = Some(error);
                        break 'run;
                    }
                }
                response.processed += 1;
                processed_progress = Some(response.processed);
            }
        }
        if script.function_exists("Finalize") && !script.function_is_empty("Finalize") {
            match call_script(&mut script, &state, "Finalize", &[]) {
                Ok(value) => {
                    apply_pending_filters(session, &state)?;
                    let result = result_as_integer(&value);
                    if result != 0 {
                        state
                            .borrow_mut()
                            .progress(&format!("Script terminated itself, Result={result}"), false);
                        self_terminated = Some(result);
                        break 'run;
                    }
                }
                Err(error) => {
                    last_error = Some(error);
                    break 'run;
                }
            }
        }
    }

    {
        let mut state = state.borrow_mut();
        match &last_error {
            Some(error) => {
                // `wbProgress('Exception' + LastErrorLocation + ': [' +
                // E.ClassName + '] ' + E.Message, True)`, then the
                // `Aborted: ` of `PerformLongAction`'s `EAbort` handler.
                state.progress(
                    &format!(
                        "Exception in {}: [{}] {}",
                        script.last_error_location(),
                        error.class_name(),
                        error.message()
                    ),
                    true,
                );
                state.progress(&format!("Aborted: {action}"), false);
            }
            None => {
                let elapsed = elapsed_text(state.started.elapsed());
                // `PerformLongAction`'s closing line: the `Processed Records`
                // segment appears only while `wbCurrentProgress` was set,
                // which `ScriptProcessSingleElement` does after every
                // processed element (`xeMainForm.pas:8704-8705`,
                // `:15296-15301`) -- a run whose `Process` never fired has
                // none.
                let done = match processed_progress {
                    Some(processed) => {
                        format!("Done: {action}, Processed Records: {processed}, Elapsed Time: {elapsed}")
                    }
                    None => format!("Done: {action}, Elapsed Time: {elapsed}"),
                };
                state.progress(&done, false);
                // `DoRunScript`'s last line; an aborted run never reaches it.
                state.add_message("You can close this application now.");
            }
        }
        if silent {
            state.progress_lock -= 1;
        }
    }
    response.aborted = last_error.is_some();
    response.self_terminated = self_terminated;
    if let Some(error) = &last_error {
        response.error = Some(error.message().to_owned());
        response.error_location = Some(script.last_error_location());
    }

    // The plugins the script changed: saved as the GUI does at the end of an
    // auto mode (`SaveChanged` of the close), unless the run was a dry run or
    // saving is off. Upstream writes `<name>.save.<yyyy_mm_dd_hh_nn_ss>` next
    // to the file, logs `Saving: <name>.save.<time>`, queues the rename over
    // the file for its shutdown (`Queued renaming of save ... on shutdown.`)
    // and closes with `Done saving.`; a completed run's lines come after
    // `You can close this application now.` (outside the compared section), an
    // aborted run's are its last lines.
    let changed_files: Vec<std::sync::Arc<xedit_core::implementation::FileImpl>> = {
        let mut changed = Vec::new();
        for file in crate::conflicts::session_files(session)? {
            if file.get_is_editable() && file.element_base().has_state(ElementState::esUnsaved) {
                changed.push(file);
            }
        }
        changed
    };
    if !changed_files.is_empty() && request.save && !request.dry_run {
        // `t := '.save.' + FormatDateTime('yyyy_mm_dd_hh_nn_ss', Now)`.
        let suffix = format!(".save.{}", save_timestamp());
        let data_path = globals::data_path();
        for file in &changed_files {
            let name = file.get_name();
            // `NeedsRename := FileExists(wbDataPath + name)`: the loaded
            // file is the copy below the data path, so it exists.
            let queued = format!("{name}{suffix}");
            state.borrow_mut().progress(&format!("Saving: {queued}"), false);
            // The port writes the file itself, without the GUI's backup
            // folder (which lives below `wbBackupPath`, outside the private
            // data folder, and is not part of what a run leaves); the queued
            // rename is applied here at once, because the process ends after
            // the run, so the file holds the new bytes as it does after the
            // GUI's shutdown rename.
            let saved = crate::save::save_file(file, None, false, false)?;
            state.borrow_mut().progress(
                &format!("Queued renaming of save \"{data_path}{queued}\" to \"{data_path}{name}\" on shutdown."),
                false,
            );
            response.files.push(ScriptRunFile::from_save(file, &saved));
        }
        state.borrow_mut().progress("Done saving.", false);
    } else {
        for file in &changed_files {
            let mut entry = ScriptRunFile::new(file);
            entry.changed = true;
            response.files.push(entry);
        }
    }
    {
        let mut state = state.borrow_mut();
        state.show_start_time -= 1;
        // Anything the relay still holds after the last state line (the
        // save's own messages go through the state already).
        state.add_message("");
        response.messages = state.messages.clone();
    }
    Ok(response)
}

/// `ApplyScript`'s `Mode: Silent` keyword: `ContainsText(Copy(aScript, p,
/// PosEx(#10, aScript, p) - p), 'Silent')` (`xeMainForm.pas:8777-8778`) with
/// `p := Pos('Mode:', aScript)` (case-sensitive). A `Mode:` on the final
/// line without a trailing `#10` gives `PosEx` = 0 and a negative `Copy`
/// count -- the empty window, so not silent.
fn is_silent(source: &[u8]) -> bool {
    let Some(at) = source.windows(5).position(|window| window == b"Mode:") else {
        return false;
    };
    // `PosEx(#10, aScript, p)` is 0 when no newline follows: `Copy` with a
    // negative count is ''.
    let Some(end) = source[at..].iter().position(|byte| *byte == b'\n') else {
        return false;
    };
    let line = String::from_utf8_lossy(&source[at..at + end]);
    line.to_ascii_lowercase().contains("silent")
}

/// `IncludeTrailingPathDelimiter`.
fn trailing_separator(path: &Path) -> String {
    let text = path.display().to_string();
    if text.ends_with(['\\', '/']) {
        text
    } else {
        format!("{text}\\")
    }
}

/// `FormatDateTime('yyyy_mm_dd_hh_nn_ss', Now)` of the save suffix. Delphi's
/// `Now` is local time; without a time zone database the port uses UTC (the
/// parity comparison normalizes the digits of the suffix, so the two runs of
/// a case match whatever the clock says).
fn save_timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64);
    let (hour, minute, second) = {
        let day = seconds.rem_euclid(86_400);
        (day / 3_600, (day / 60) % 60, day % 60)
    };
    // The civil date of the days since the epoch (Howard Hinnant's
    // `civil_from_days`).
    let z = seconds.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 {
        yoe + era * 400 + 1
    } else {
        yoe + era * 400
    };
    format!("{year:04}_{month:02}_{day:02}_{hour:02}_{minute:02}_{second:02}")
}

/// `script.run`: the command handler, over the GUI's selection.
fn script_run(session: &mut Session, request: ScriptRunRequest) -> Result<ScriptRunResponse, CommandError> {
    script_run_gated(session, request, &mut NavigationSelection)
}

#[cfg(test)]
mod tests {
    use super::is_silent;

    /// `Copy(aScript, p, PosEx(#10, aScript, p) - p)` (`xeMainForm.pas:8778`)
    /// is the window from `Mode:` to the next `#10`; without a newline
    /// `PosEx` is 0 and `Copy` with the negative count is the empty string,
    /// which `ContainsText` never matches.
    #[test]
    fn the_silent_keyword_is_the_window_of_the_mode_line() {
        assert!(is_silent(b"unit u;\n// Mode: Silent\nbegin\nend.\n"));
        assert!(is_silent(b"Mode: silent\n"));
        // No newline after the keyword: the empty window.
        assert!(!is_silent(b"unit u;\nbegin\nend.\n// Mode: Silent"));
        assert!(!is_silent(b"Mode: Silent"));
        // No `Mode:` at all; a window that does not say Silent.
        assert!(!is_silent(b"unit u;\nend.\n"));
        assert!(!is_silent(b"Mode: Verbose\n"));
    }
}
