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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_analysis::filter::{ScriptedFilter, ScriptedFilterError};
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
        "Run a Pascal script over the loaded plugins as xEdit's -script: mode does (DoRunScript/ApplyScript): Initialize, Process per selected element, Finalize, then save the plugins the script changed. Needs --edit unless --dry-run. `answers` supplies recorded answers to the script's dialogs (the headless VCL model of script_vcl::interactive); a dialog without an answer stops the run with `pending_interaction` in the response.",
        true,
        script_run,
    );
    registry.register(
        "script.resume",
        "Continue a script run that stopped on a dialog (`pending_interaction`): the same parameters as script.run, with the accumulated answers plus one for the reported question. The interpreter has no continuation, so the script runs from its start again with the answers supplied so far; a deterministic script reaches the same state and proceeds past the interaction.",
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
    /// Recorded answers to the dialogs the script shows, in the order it
    /// shows them (the same JSON the parity harness gives the oracle GUI).
    /// A dialog with no remaining answer stops the run with a
    /// `pending_interaction` in the response; re-invoke with the answer
    /// appended (`script.resume`) to continue.
    #[serde(default)]
    pub answers: Vec<ScriptAnswer>,
}

/// One recorded answer to a dialog a script shows (`script_vcl::interactive`
/// consumes them in order; the oracle harness's `Answer` has the same core
/// fields and may carry extra ones, which are ignored).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct ScriptAnswer {
    /// The Delphi window class of the dialog: `#32770` for `MessageDlg`,
    /// `ShowMessage`, `MessageBox` and the file dialogs, `TInputQueryForm`
    /// for an `InputQuery`/`InputBox`, the form's class name otherwise. Must
    /// equal the shown dialog's class.
    pub class: String,
    /// A substring of the dialog's caption.
    pub title: String,
    /// Virtual-key codes posted to the dialog's focused control, in order
    /// (the oracle harness's `keys`).
    #[serde(default)]
    pub keys: Vec<u16>,
    /// The button to click, by its caption (`&Yes`, `OK`, `&Find all
    /// records`); `&` is ignored in the comparison.
    #[serde(default)]
    pub button: Option<String>,
    /// Text put into the dialog's edit control before the keys (`WM_SETTEXT`
    /// on the first edit, as the oracle harness does): the value an
    /// `InputQuery`/`InputBox` returns, the file a selection dialog picks
    /// (a name without a drive or a root is taken under the dialog's
    /// `InitialDir`, as the common dialog resolves it).
    #[serde(default)]
    pub text: Option<String>,
    /// The class of the form's child control the text and the keys go to
    /// (`TCheckListBox`, `TCheckBox`, `TLabeledEdit`) in place of the first
    /// edit and the active control: how a form with several controls is
    /// driven control by control, each answer naming its control and
    /// `repeat`.
    #[serde(default)]
    pub control: Option<String>,
    /// The answer goes to the same form as the answer before it: a form a
    /// scenario drives control by control is one window with several
    /// answers, each after the first naming `repeat` (the parity harness's
    /// rule, so one answers file drives the oracle GUI and the port).
    #[serde(default)]
    pub repeat: bool,
}

/// The dialog a run stopped on, reported as `script.run`'s
/// `pending_interaction`: answer it by re-invoking with the answers
/// accumulated plus one for this question.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ScriptInteraction {
    /// `message` (`MessageDlg`/`MessageBox`/`ShowMessage`), `input`
    /// (`InputQuery`/`InputBox`), `file` (`TOpenDialog`/`TSaveDialog`) or
    /// `form` (a script-built form shown modal).
    pub kind: String,
    /// The Delphi window class a `ScriptAnswer::class` must equal.
    pub class: String,
    /// The dialog's caption; a `ScriptAnswer::title` must be a substring of
    /// it.
    pub title: String,
    /// The message or prompt of the dialog.
    pub text: String,
    /// The labels of the buttons the dialog offers, `&` included (`&Yes`).
    pub buttons: Vec<String>,
    /// The label of the button the default action would click.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
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
    /// The dialog the run stopped on: the script's next question, when the
    /// recorded `answers` ran out (`EVclInteractionRequired`). Re-invoke with
    /// the answers accumulated plus one for this question (`script.resume`)
    /// and the run continues past it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_interaction: Option<ScriptInteraction>,
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

    /// The roots the walk runs over ([`selection`](Self::selection)).
    ///
    /// `DoRunScript` selects every root node of the navigation tree (all
    /// loaded files) and `ApplyScriptToSelection` walks each selected node's
    /// subtree *backwards*: it starts at `vstNav.GetLast(node)` and steps
    /// `GetPrevious` until the node itself, so the last element of a file is
    /// processed first and the file node last. With
    /// [`live_walk`](Self::live_walk) the steps happen over the navigation
    /// tree as `vstNav` builds it lazily; without it the returned elements
    /// are the walk's fixed order.
    fn selection(&mut self, session: &mut Session, files: &[ElementRef]) -> Result<Vec<ElementRef>, CommandError>;

    /// Whether the walk steps over the navigation tree (`vstNav.GetLast` /
    /// `GetPrevious`): a node's children are its container's elements when
    /// the walk first needs them and a child binds its element when the walk
    /// reaches it (see [`WalkNode`]), so a record a `Process` moves is
    /// visited again only where the walk reaches a container it has not
    /// enumerated yet (`Processed Records` above the record count; the
    /// issue-#24 `walk-move` probe and the two Worldspace cases pin it), and
    /// a group created under a container the walk has already enumerated is
    /// never reached (issue #84).
    fn live_walk(&self) -> bool {
        false
    }
}

/// The port of the navigation tree walk: every loaded file is selected as a
/// root, and each root's subtree is walked backwards over the navigation
/// tree ([`live_walk`](ScriptSelection::live_walk)).
///
/// The tree holds the file, its groups and its records (`vstNavInitChildren`
/// gives every container element a node) and, for a record, the child group
/// that holds the record's children (`vstNavInitNode`: a main record's node
/// container is `ChildGroup`, not the record's own elements), so the walk
/// reaches records through their groups. The default filter of a run is
/// `[etMainRecord]` (`ApplyScript`), so with no script variable set it is
/// every main record of the loaded files, each file's records in reverse
/// FormID order (the file's `flRecords` order), with the re-visits a moved
/// record gets where the walk reaches a container it has not enumerated
/// yet.
#[derive(Default)]
pub struct NavigationSelection;

impl ScriptSelection for NavigationSelection {
    fn files(&mut self, session: &mut Session) -> Result<Vec<ElementRef>, CommandError> {
        let files = crate::conflicts::session_files(session)?;
        Ok(files.iter().filter_map(|file| file.self_element_ref()).collect())
    }

    fn selection(&mut self, _session: &mut Session, files: &[ElementRef]) -> Result<Vec<ElementRef>, CommandError> {
        Ok(files.to_vec())
    }

    fn live_walk(&self) -> bool {
        true
    }
}

/// The parent link of a file node: a root of the walk has no parent node.
const NO_PARENT: usize = usize::MAX;

/// One node of the navigation tree `vstNav` builds lazily for one script
/// walk, with the state `vstNavInitChildren` and `vstNavInitNode` give it.
///
/// UPSTREAM-QUIRK: the tree freezes a container's child count when the walk
/// first needs the children and binds a child's element only when the walk
/// reaches it, so the walk does not mirror the element lists it walks:
///
/// - `vstNavInitChildren` fixes a node's child count the first time the
///   walk needs the node's children, so an element a script adds to that
///   container afterwards has no node and is never visited by this walk
///   (the issue-#84 `persistent-walk.pas`: the persistent children group a
///   `SetIsPersistent` creates under a cell whose children the walk has
///   already enumerated is never reached).
/// - `vstNavInitNode` binds a child's element only when the walk reaches
///   it, from the container's element at the child's slot *then*, so an
///   element that moves into a container the walk has not enumerated yet
///   is found where it is now when the walk gets there (`walk-move.pas`
///   re-visits `REFR 0000080A` in its new cell).
/// - A record that moves keeps the node it had and the place it had: the
///   walk never sets foot under its old parent again.
#[derive(Clone)]
struct WalkNode {
    /// The parent node, [`NO_PARENT`] for a file node.
    parent: usize,
    /// The node's slot among its parent's children -- the element index
    /// `vstNavInitNode` reads `Container.Elements[Node.Index]` from.
    index: u32,
    /// Whether `vstNavInitNode` has run for this node.
    bound: bool,
    /// `NodeData.Element`, weakly: a node must not count as a reference of
    /// its element (`ElementStates` reads the port's strong counts where
    /// upstream's tree holds an `IwbElement` that leaves `cntElementRefs`
    /// alone, as the position index below notes).
    element: Option<WeakElement>,
    /// `NodeData.Container`: the element whose element list supplies this
    /// node's children -- the element itself for a file or group, the child
    /// group for a main record.
    container: Option<ElementRef>,
    /// `vsHasChildren` after `vstNavInitNode`.
    has_children: bool,
    /// `ChildCount` after `vstNavInitChildren`; `None` until the walk first
    /// needs the children.
    child_count: Option<u32>,
    /// `vstNav.DeleteNode`: a `RemoveNode` took the node with its element.
    removed: bool,
}

/// The navigation tree of the walk in progress: its nodes and, for a
/// `RemoveNode`, the nodes each bound element was bound to, by the element's
/// allocation address (a weak reference keeps the allocation from being
/// reused while the node holds it).
#[derive(Default)]
struct WalkTree {
    nodes: Vec<WalkNode>,
    bound: HashMap<usize, Vec<usize>>,
}

thread_local! {
    /// The navigation tree of the walk in progress (cleared at the start and
    /// the end of every walk, [`clear_walk_tree`]).
    static WALK_TREE: std::cell::RefCell<WalkTree> = std::cell::RefCell::new(WalkTree::default());
}

/// Drops the nodes of the walk in progress.
fn clear_walk_tree() {
    WALK_TREE.with(|tree| *tree.borrow_mut() = WalkTree::default());
}

/// The node of a selected root (a file): `vstNavInitNode` ran for it when it
/// was added to the tree, with the file as its container.
fn walk_root(root: &ElementRef) -> usize {
    WALK_TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        let has_children = root
            .as_element_impl()
            .and_then(|element| element.container_base())
            .is_some_and(|base| base.element_count() > 0);
        tree.nodes.push(WalkNode {
            parent: NO_PARENT,
            index: 0,
            bound: true,
            element: Some(Arc::downgrade(root)),
            container: Some(root.clone()),
            has_children,
            child_count: None,
            removed: false,
        });
        let id = tree.nodes.len() - 1;
        tree.bound.entry(element_address(root)).or_default().push(id);
        id
    })
}

/// The child node of `parent`'s slot `index`. The walk reaches every slot
/// once: it enters a subtree at its last node and leaves it at the subtree's
/// root, so no slot is created twice.
fn child_slot(tree: &mut WalkTree, parent: usize, index: u32) -> usize {
    tree.nodes.push(WalkNode {
        parent,
        index,
        bound: false,
        element: None,
        container: None,
        has_children: false,
        child_count: None,
        removed: false,
    });
    tree.nodes.len() - 1
}

/// `vstNavInitNode`: bind the node's element, its child container and, for a
/// parented group that is the child group of the record before it, the
/// hidden state (`NodeData.Element`/`Container := nil`, no children).
fn init_node(tree: &mut WalkTree, id: usize) {
    if tree.nodes[id].bound {
        return;
    }
    tree.nodes[id].bound = true;
    let parent = tree.nodes[id].parent;
    if parent == NO_PARENT {
        return;
    }
    let index = tree.nodes[id].index as usize;
    let Some(container) = tree.nodes[parent].container.clone() else {
        return;
    };
    let Some(base) = container.as_element_impl().and_then(|element| element.container_base()) else {
        return;
    };
    // `Container.Elements[Node.Index]`: the element in the slot at the
    // moment the walk reaches the node, not the element the container held
    // when its children were enumerated (`vstNavInitNode`).
    let Some(element) = base.element_at(index) else {
        return; // ivsHidden: a slot the container no longer holds
    };
    if element.get_element_type() == ElementType::etGroupRecord
        && let Some(group) = element.as_element_impl().and_then(ElementImpl::group_record_impl)
        && is_parented_group(group.group_type())
        && hidden_group_record(base, index).is_some()
    {
        // `vstNavInitNode`'s tail: a parented group that is the child group
        // of the record before it is hidden and carries no element or
        // children of its own; the record's node shows its children.
        return;
    }
    tree.nodes[id].element = Some(Arc::downgrade(&element));
    tree.bound.entry(element_address(&element)).or_default().push(id);
    if element.get_element_type() == ElementType::etMainRecord {
        // `vstNavInitNode`: the parented group right after the record in its
        // container, labelled with the record's FormID, is the container
        // its node enumerates as children.
        if let Some(next) = base.element_at(index + 1)
            && let Some(group) = next.as_element_impl().and_then(ElementImpl::group_record_impl)
            && is_parented_group(group.group_type())
            && element
                .as_element_impl()
                .and_then(ElementImpl::main_record_impl)
                .is_some_and(|record| group.get_group_label() == record.mr_struct().form_id.to_cardinal())
        {
            tree.nodes[id].has_children = next
                .as_element_impl()
                .and_then(|element| element.container_base())
                .is_some_and(|base| base.element_count() > 0);
            tree.nodes[id].container = Some(next);
        }
    } else if let Some(inner) = element.as_element_impl().and_then(|element| element.container_base())
        && inner.element_count() > 0
    {
        tree.nodes[id].has_children = true;
        tree.nodes[id].container = Some(element.clone());
    }
}

/// `vstNavInitChildren`: `ChildCount := Container.ElementCount`, read the
/// first time the walk needs the node's children.
fn child_count(tree: &mut WalkTree, id: usize) -> u32 {
    if let Some(count) = tree.nodes[id].child_count {
        return count;
    }
    let count = if tree.nodes[id].removed || !tree.nodes[id].has_children {
        0
    } else {
        tree.nodes[id]
            .container
            .as_ref()
            .and_then(|container| container.as_element_impl())
            .and_then(|element| element.container_base())
            .map(|base| base.element_count() as u32)
            .unwrap_or(0)
    };
    tree.nodes[id].child_count = Some(count);
    count
}

/// `vstNav.GetLast(aNode)`: the last node of `root`'s subtree. Each node on
/// the way down is initialized (`vstNavInitNode`) and a node with children
/// has them enumerated (`vstNavInitChildren`) and is entered through its
/// last child (`GetLastChild`).
fn last_node(root: usize) -> usize {
    WALK_TREE.with(|tree| last_in(&mut tree.borrow_mut(), root))
}

fn last_in(tree: &mut WalkTree, root: usize) -> usize {
    let mut node = root;
    loop {
        init_node(tree, node);
        let count = child_count(tree, node);
        if count == 0 {
            return node;
        }
        node = child_slot(tree, node, count - 1);
    }
}

/// `vstNav.GetPrevious(aNode)`: the previous node of the pre-order of the
/// node tree -- the last node of the previous sibling's subtree, or the
/// parent when the node is its first child. `None` when the node has no
/// parent (a file node, the walk's root). The parent is the predecessor even
/// when it is the walk's root: the root is processed last and the walk stops
/// there.
fn previous_node(node: usize) -> Option<usize> {
    WALK_TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        let current = tree.nodes.get(node)?.clone();
        if current.parent == NO_PARENT {
            return None;
        }
        if current.index == 0 {
            return Some(current.parent);
        }
        let sibling = child_slot(&mut tree, current.parent, current.index - 1);
        Some(last_in(&mut tree, sibling))
    })
}

/// `NodeData.Element`, when the node still has one: a hidden child group, a
/// slot the container no longer holds and a node a `RemoveNode` deleted all
/// process nothing (`ScriptCanProcessElement(nil)`).
fn walk_element(node: usize) -> Option<ElementRef> {
    WALK_TREE.with(|tree| {
        let tree = tree.borrow();
        let node = tree.nodes.get(node)?;
        if node.removed {
            return None;
        }
        node.element.as_ref().and_then(std::sync::Weak::upgrade)
    })
}

/// `_RemoveNode`'s `vstNav.DeleteNode(Node)`: the element's node leaves the
/// tree with it (`NodeData.Element := nil`). The walk may still hold the
/// node it stepped to; it processes nothing there, and its links still
/// navigate backwards as upstream's freed node's do.
fn walk_remove_element(element: &ElementRef) {
    let address = element_address(element);
    WALK_TREE.with(|tree| {
        let mut tree = tree.borrow_mut();
        // A weak reference keeps the allocation alive, so the address names
        // this element alone for the whole walk.
        let Some(nodes) = tree.bound.get(&address).cloned() else {
            return;
        };
        for node in nodes {
            tree.nodes[node].removed = true;
        }
    });
}

/// The parent node of `node` in the tree: the container element, or, for a
/// node inside a parented group that is the hidden child group of the
/// record before it, that record (`vstNavInitNode`: a record's node
/// container is its child group).
fn tree_parent(node: &ElementRef) -> Option<ElementRef> {
    let container = node.as_element_impl()?.element_base().container()?;
    let container_impl = container.as_element_impl()?;
    if let Some(group) = container_impl.group_record_impl()
        && is_parented_group(group.group_type())
        && let Some(record) = parent_record_of_hidden_group(&group)
    {
        return Some(record);
    }
    Some(container)
}

/// The record a hidden parented group belongs to: the element before the
/// group in its container, when it is a main record with the group's label
/// (`xeMainForm.pas:20299-20347`).
fn parent_record_of_hidden_group(group: &Arc<xedit_core::implementation::GroupRecordImpl>) -> Option<ElementRef> {
    let group_ref = group.self_element_ref()?;
    let container = group_ref.as_element_impl()?.element_base().container()?;
    let base = container.as_element_impl()?.container_base()?;
    let index = position_in(&container, &group_ref)?;
    hidden_group_record(base, index)
}

/// The record whose hidden child group the element at `index` of a
/// container is: a parented group right after a main record whose FormID is
/// the group's label.
fn hidden_group_record(base: &xedit_core::implementation::ContainerBase, index: usize) -> Option<ElementRef> {
    let element = base.element_at(index)?;
    let group = element.as_element_impl().and_then(ElementImpl::group_record_impl)?;
    if !is_parented_group(group.group_type()) {
        return None;
    }
    let before = base.element_at(index.checked_sub(1)?)?;
    let record = before.as_element_impl().and_then(ElementImpl::main_record_impl)?;
    (group.get_group_label() == record.mr_struct().form_id.to_cardinal()).then_some(before)
}

/// An element or container held weakly: the tree must not count as a
/// reference of it (the resets of upstream's `cntElementRefs`, which
/// `ContainerBase::is_element_referenced` reads from the strong counts, see
/// the same elements with and without the walk), while the allocation the
/// weak reference keeps stops the address from naming another element.
type WeakElement = std::sync::Weak<dyn xedit_core::interface::Element>;

/// The position index of one container: the container (weakly), the
/// generation of its element list the index was read at, and the position
/// of each element by its address.
type WalkPositions = (WeakElement, u32, std::rc::Rc<HashMap<usize, usize>>);

thread_local! {
    /// The positions of the elements of the containers the walk has stepped
    /// through, by the container's address, with the generation of its
    /// element list they were read at and the container (weakly). Cleared at
    /// the start and the end of every walk ([`clear_walk_positions`]).
    static WALK_POSITIONS: std::cell::RefCell<HashMap<usize, WalkPositions>> = std::cell::RefCell::new(HashMap::new());
}

fn element_address(element: &ElementRef) -> usize {
    Arc::as_ptr(element).cast::<()>() as usize
}

/// The position of `element` among the elements of `container`, from an
/// index that is read again only when the container's element list changed
/// (its generation, `ContainerBase::generation`).
fn position_in(container: &ElementRef, element: &ElementRef) -> Option<usize> {
    let base = container.as_element_impl()?.container_base()?;
    let key = element_address(container);
    let target = element_address(element);
    let generation = base.generation();
    let cached = WALK_POSITIONS.with(|cache| {
        cache
            .borrow()
            .get(&key)
            .filter(|(held, at, _)| *at == generation && held.strong_count() > 0)
            .map(|(_, _, positions)| positions.clone())
    });
    if let Some(positions) = cached {
        return positions.get(&target).copied();
    }
    let positions: HashMap<usize, usize> = base
        .elements()
        .iter()
        .enumerate()
        .map(|(index, element)| (element_address(element), index))
        .collect();
    let found = positions.get(&target).copied();
    // A change while the list was read leaves the index unkept.
    if base.generation() == generation {
        WALK_POSITIONS.with(|cache| {
            cache.borrow_mut().insert(
                key,
                (Arc::downgrade(container), generation, std::rc::Rc::new(positions)),
            );
        });
    }
    found
}

/// Drops the position index of [`position_in`] and the containers it holds.
fn clear_walk_positions() {
    WALK_POSITIONS.with(|cache| cache.borrow_mut().clear());
}

/// The children of a node, with the hidden parented groups folded into the
/// record before them: the reference the tests compare the walk against on a
/// tree no `Process` changes. The walk itself steps over the raw slots
/// ([`last_node`], [`previous_node`]) and binds each child when it reaches
/// it; a hidden group's slot is stepped over there because it carries no
/// element.
#[cfg(test)]
fn node_children(node: &ElementRef) -> Vec<ElementRef> {
    let Some(node_impl) = node.as_element_impl() else {
        return Vec::new();
    };
    if let Some(record) = node_impl.main_record_impl() {
        let Some(container) = node_impl.element_base().container() else {
            return Vec::new();
        };
        let Some(base) = container
            .as_element_impl()
            .and_then(|container| container.container_base())
        else {
            return Vec::new();
        };
        let elements = base.elements();
        let Some(index) = elements
            .iter()
            .position(|element| std::ptr::addr_eq(Arc::as_ptr(element), Arc::as_ptr(node)))
        else {
            return Vec::new();
        };
        if let Some(next) = elements.get(index + 1)
            && let Some(group) = next.as_element_impl().and_then(ElementImpl::group_record_impl)
            && is_parented_group(group.group_type())
            && group.get_group_label() == record.mr_struct().form_id.to_cardinal()
        {
            return tree_children_of_container(next);
        }
        return Vec::new();
    }
    tree_children_of_container(node)
}

/// The element children of a file or group container, in order, with a
/// parented group whose label is the FormID of the record before it left
/// out (its records are reached through that record, exactly once).
#[cfg(test)]
fn tree_children_of_container(container: &ElementRef) -> Vec<ElementRef> {
    let mut result = Vec::new();
    let Some(base) = container.as_element_impl().and_then(|node| node.container_base()) else {
        return result;
    };
    let elements = base.elements();
    let mut index = 0;
    while index < elements.len() {
        let child = &elements[index];
        if child.get_element_type() == ElementType::etMainRecord {
            result.push(child.clone());
            index += 1;
            if let Some(next) = elements.get(index)
                && let Some(group) = next.as_element_impl().and_then(ElementImpl::group_record_impl)
                && is_parented_group(group.group_type())
                && let Some(record) = child.as_element_impl().and_then(ElementImpl::main_record_impl)
                && group.get_group_label() == record.mr_struct().form_id.to_cardinal()
            {
                index += 1;
            }
            continue;
        }
        result.push(child.clone());
        index += 1;
    }
    result
}

/// `ParentedGroupRecordType` (`xeMainForm.pas:5157`): world, cell and topic
/// children, quest children with `wbVWDAsQuestChildren`.
/// The element whose navigation node an element shows as: a child group is
/// shown by its record's node (`NodeData.Container`), any other element by
/// its own (`FindNodeForElement` matches either).
pub(crate) fn node_element(element: &ElementRef) -> ElementRef {
    if element.get_element_type() == ElementType::etGroupRecord
        && let Some(group) = element.as_element_impl().and_then(ElementImpl::group_record_impl)
        && is_parented_group(group.group_type())
        && let Some(record) = parent_record_of_hidden_group(&group)
    {
        return record;
    }
    element.clone()
}

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
    /// The main record the element is a member of (at any depth), whose
    /// count of references to its members this value is one of
    /// ([`xedit_core::implementation::MainRecordImpl::acquire_member_ref`]).
    record: Option<std::sync::Weak<dyn xedit_core::interface::Element>>,
}

impl ScriptElement {
    /// The value of `element`, counted as a reference into its main record.
    fn new(element: &ElementRef, class: Rc<ClassDef>) -> Self {
        let record = owning_main_record(element);
        if let Some(record) = record
            .as_ref()
            .and_then(|record| record.as_element_impl()?.main_record_impl())
        {
            record.acquire_member_ref();
        }
        ScriptElement {
            element: element.clone(),
            class,
            record: record.as_ref().map(std::sync::Arc::downgrade),
        }
    }

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

/// The main record `element` is a member of, at any depth; `None` for a
/// main record itself and for what is no member of one (a file, a group).
fn owning_main_record(element: &ElementRef) -> Option<ElementRef> {
    if element.get_element_type() == ElementType::etMainRecord {
        return None;
    }
    let mut current = element.get_container()?;
    loop {
        match current.get_element_type() {
            ElementType::etMainRecord => return Some(current),
            ElementType::etFile | ElementType::etGroupRecord => return None,
            _ => current = current.get_container()?,
        }
    }
}

impl Drop for ScriptElement {
    /// The release of the script's reference (`_Release` upstream). A value
    /// whose last reference this was, with nothing below it referenced,
    /// resets (`TwbContainer._Release`: `cntElementRefs = 0`, and
    /// `CanElementReset` of a value is `eExternalRefs < 1`; `DoReset` exits
    /// on `esModified`). The reference into the main record goes; the last
    /// one resets the record, which [`xedit_core::implementation::MainRecordImpl::acquire_member_ref`]
    /// accounts for when the next one is taken.
    fn drop(&mut self) {
        // The parent's reference and this one.
        let last = std::sync::Arc::strong_count(&self.element) <= 2;
        if let Some(element) = self.element.as_element_impl()
            && last
            && element.value_impl().is_some()
            && !element.element_base().has_state(ElementState::esModified)
            && let Some(container) = element.container_base()
            && container.is_init_live()
            && !container.is_subtree_referenced()
        {
            container.stamp_reset();
        }
        if let Some(record) = self.record.as_ref().and_then(std::sync::Weak::upgrade)
            && let Some(record) = record.as_element_impl().and_then(|record| record.main_record_impl())
        {
            record.release_member_ref();
        }
    }
}

impl ScriptObject for ScriptElement {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    /// Two element values of one element are equal (the interface pointer
    /// identity of an `IInterface` variant upstream).
    fn identity(&self) -> Option<u64> {
        Some(self.element.get_element_id() as u64)
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
    /// `FilterScripted` (`cbScriptedFilter`): the filter calls the script's
    /// `Filter` function for every main record it reaches ([`RunScriptedFilter`]).
    /// Upstream clears the flag when the filter ends, whatever path it exits
    /// by; the run loop does the same after [`apply_pending_filters`].
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
            scripted: self.scripted,
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
    /// The elements of the nodes the last applied filter left, or `None`
    /// while no filter is on the tree: `ApplyScriptToSelection` walks the
    /// live tree, so after a filter the `Process` walk reaches only the
    /// nodes the filter kept (`RemoveFilter`'s `ReInitTree` returns to
    /// `None`).
    pub filtered_nodes: Option<std::collections::HashSet<usize>>,
    /// The `Process` walk of the filtered tree, by the identity of each
    /// root's element, while a filter is on the tree
    /// ([`crate::filter::AppliedFilter::walk`]): the walk follows the nodes
    /// the filter bound, which keep a topic's responses in the order the
    /// filter's walk found them.
    pub filtered_walk: Option<std::collections::HashMap<usize, Vec<ElementRef>>>,
    /// The filter call the script asked for; the run loop runs it with the
    /// session when the call that asked returns (see [`PendingFilter`]). A
    /// second call replaces the first: what a filter leaves on the tree is
    /// its last state.
    pub pending_filter: Option<PendingFilter>,
    /// The clock the `[mm:ss]` prefixes of the log count from: `wbStartTime`,
    /// which `xeInit.pas` sets when the plugin list load starts and
    /// `BuildAllRef` re-sets while the references are built (see
    /// [`reset_progress_start`]).
    started: std::time::Instant,
    /// When the `Applying script` action began, for the `Done:` line's
    /// `Elapsed Time`: `PerformLongAction` counts from its own start, which
    /// is also when it posts `Start: Applying script`.
    action_started: std::time::Instant,
    /// The script list objects the adapter steps append to (`TList`,
    /// `TStrings`); none until the JVCL `Classes` unit registers them.
    pub lists: ScriptLists,
    /// The relay of the process-wide progress callback: everything a run
    /// logs belongs in the one memo (`mmoMessages`), and that callback
    /// cannot hold this state, so [`run_script`] installs a relay whose
    /// lines [`ScriptRunState::add_message`] drains in order.
    relayed: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// The main form's state the `xejviScriptAdapterHost.pas` registrations
    /// of issue #25 read and change: the navigation tree's selection, the
    /// conflict status its nodes hold, the cached settings and record
    /// definition map objects.
    pub host: crate::script_host::HostState,
}

/// One `ApplyFilter`/`RemoveFilter` a script called. The adapter function
/// cannot hold the session, so it leaves the request here and the run loop
/// applies it as soon as the call into the script returns.
pub enum PendingFilter {
    /// The request is boxed: it carries the whole filter dialog
    /// (`FilterRequestOptions`), and `Remove` has no data. `at` is the
    /// length of the message log when `ApplyFilter` was called: the filter
    /// runs inside that call upstream, so its lines belong there, before
    /// the messages of the statements that follow the call.
    Apply {
        request: Box<crate::filter::FilterApplyRequest>,
        at: usize,
    },
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
            filtered_nodes: None,
            filtered_walk: None,
            pending_filter: None,
            started: progress_start(),
            action_started: std::time::Instant::now(),
            lists: ScriptLists::default(),
            relayed: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            host: crate::script_host::HostState::default(),
        }
    }

    /// The script value of an element: the `IInterface` value the element API
    /// functions take (see [`ScriptElement`]).
    pub fn element_value(&self, element: &ElementRef) -> Value {
        Value::Object(Rc::new(RefCell::new(ScriptElement::new(
            element,
            self.element_class.clone(),
        ))))
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
        if let Some(line) = self.progress_line(text, force) {
            self.add_message(&line);
        }
    }

    /// The line [`progress`](ScriptRunState::progress) writes, without
    /// adding it: the filter's application moves it (and the lines of the
    /// walk) back to the position of the `ApplyFilter` call.
    pub fn progress_line(&self, text: &str, force: bool) -> Option<String> {
        if text.is_empty() || (self.progress_lock > 0 && !force) {
            return None;
        }
        // `PostAddMessage` hands the text to `AddMessage`, which splits it
        // into the lines of the log; the prefix is on the whole text.
        Some(if self.show_start_time > 0 && self.hide_start_time < 1 {
            let elapsed = elapsed_text(self.started.elapsed());
            format!("[{elapsed}] {text}")
        } else {
            text.to_owned()
        })
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

    /// `ApplyScriptToSelection` walks the live tree, so a node a filter took
    /// out is not visited: with a filter on the tree the walk reaches the
    /// nodes the filter kept alone.
    pub fn is_filtered_out(&self, element: &ElementRef) -> bool {
        self.filtered_nodes
            .as_ref()
            .is_some_and(|nodes| !nodes.contains(&element.get_element_id()))
    }

    /// `Length(frmMain.Files)`.
    pub(crate) fn file_count(&self) -> usize {
        self.files.len()
    }

    /// `frmMain.Files`, in its order.
    pub(crate) fn files(&self) -> &[ElementRef] {
        &self.files
    }

    /// `SetLength(Files, Succ(Length(Files))); Files[High(Files)] := ...`:
    /// a file the script made joins the list (`AddNewFileName`).
    pub(crate) fn push_file(&mut self, file: ElementRef) {
        self.files.push(file);
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

/// `wbStartTime` (`xeInit.pas:5624`, `BuildAllRef`): the clock the `[mm:ss]`
/// prefixes of `wbProgress` count from. The GUI sets it just before the
/// plugin list load starts and again when the reference information is
/// built; the port's `Session::load` and `--build-refs` call
/// [`reset_progress_start`], so a prefix is the time since the load began,
/// as the oracle's is.
static PROGRESS_START: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// `wbStartTime := Now`.
pub(crate) fn reset_progress_start() {
    *PROGRESS_START.lock().unwrap() = Some(std::time::Instant::now());
}

/// The `[mm:ss]` clock; a clock that was never reset counts from now.
pub(crate) fn progress_start() -> std::time::Instant {
    PROGRESS_START.lock().unwrap().unwrap_or_else(std::time::Instant::now)
}

/// `wbFormatElapsedTime` (`xeMainForm.pas:1361`): `FormatDateTime('nn:ss')`,
/// the hours in front once there are any.
pub(crate) fn elapsed_text(elapsed: std::time::Duration) -> String {
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
/// `vcl` is the VCL seam: the four `JvInterpreterFm.pas` registrations, the
/// members of the ten VCL-backed JVCL units and
/// `xejviScriptAdapterMiscVcl.pas` route through it. A script run passes
/// [`crate::script_vcl::interactive::VclModel`] (issue #26); the callers
/// without a model (tests, standalone registrations) pass
/// [`crate::script_rtl::NoVcl`].
pub(crate) fn register_functions(
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

    //=== The LOD and atlas builders (xejviScriptAdapterHost.pas) =============

    {
        let state = state.clone();
        host_function(
            adapter,
            "wbBuildAtlasFromAtlasMap",
            5,
            vec![
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
            ],
            Rc::new(move |_value: &mut Value, args: &mut Args| {
                let Some(map) = crate::script_host::lod_list(&args.values[0]) else {
                    return Err(ValueError::interp(IE_DIRECT_INVALID_ARGUMENT));
                };
                let brightness = xedit_script::values::to_i32(&args.values[1])
                    .map_err(|_| ValueError::interp(IE_DIRECT_INVALID_ARGUMENT))?;
                let single = |value: &Value| {
                    xedit_script::values::to_f64(value)
                        .map(|value| value as f32)
                        .map_err(|_| ValueError::interp(IE_DIRECT_INVALID_ARGUMENT))
                };
                let gamma = (
                    single(&args.values[2])?,
                    single(&args.values[3])?,
                    single(&args.values[4])?,
                );
                let env = crate::script_host::lod_env(&state);
                xedit_assets::lod::build_atlas_from_atlas_map(&env, &map, brightness, gamma)
                    .map_err(|error| ValueError::external_class("Exception", error.0))?;
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        host_function(
            adapter,
            "wbBuildAtlasFromTexturesList",
            7,
            vec![
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
                ParamType::value(VarType::Empty),
            ],
            Rc::new(move |_value: &mut Value, args: &mut Args| {
                let Some(textures) = crate::script_host::lod_list(&args.values[0]) else {
                    return Err(ValueError::interp(IE_DIRECT_INVALID_ARGUMENT));
                };
                let integer = |value: &Value| {
                    xedit_script::values::to_i32(value).map_err(|_| ValueError::interp(IE_DIRECT_INVALID_ARGUMENT))
                };
                let (max_texture_size, max_tile_size) = (integer(&args.values[1])?, integer(&args.values[2])?);
                let (width, height) = (integer(&args.values[3])?, integer(&args.values[4])?);
                let string = |value: &Value| match value {
                    Value::Str(text) => xedit_io::encoding::ansi_string(text),
                    _ => String::new(),
                };
                let (name, map_name) = (string(&args.values[5]), string(&args.values[6]));
                let env = crate::script_host::lod_env(&state);
                xedit_assets::lod::build_atlas_from_textures_list(
                    &env,
                    &textures,
                    max_texture_size,
                    max_tile_size,
                    width,
                    height,
                    &name,
                    &map_name,
                )
                .map_err(|error| ValueError::external_class("Exception", error.0))?;
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
                // `SameText(Args.Values[0], frmMain.Files[i].FileName)`: the
                // file name, without the `[NN] ` of `Name` (the port compared
                // the display name, so no plain file name ever matched).
                for file in &state.files {
                    if file
                        .get_file()
                        .is_some_and(|loaded| crate::script_host::file_name_of(&loaded).eq_ignore_ascii_case(&name))
                    {
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
            // tree work has no headless counterpart beyond the node leaving
            // the walk's tree with the element ([`walk_remove_element`]).
            walk_remove_element(&element);
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
                    Some(PendingFilter::Apply { .. }) => true,
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
                let mut state = state.borrow_mut();
                // The relay's lines first, so the recorded position is the
                // one the filter's lines belong at.
                state.add_message("");
                let at = state.messages.len();
                let options = state.variables.filter_options();
                state.pending_filter = Some(PendingFilter::Apply {
                    request: Box::new(crate::filter::FilterApplyRequest {
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
                    }),
                    at,
                });
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

    //=== The rest of xejviScriptAdapterHost.pas (issue #25) ==================
    // After the VCL surface: `frmMain`/`frmFileSelect` are its objects, and
    // `wbSettings`/`wbRecordDefMap` objects of its `TMemIniFile`/`TStringList`.

    crate::script_host::register(adapter, state, &vcl);
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
/// A value error of a host hook as the interpreter error it raises (the
/// evaluator's own `value_error`, without a position: the hook's caller
/// sets it).
fn hook_error(error: ValueError) -> ExecError {
    match error {
        ValueError::Variant => ExecError::new(IE_TYPE_MISMATCH, None, "", ""),
        ValueError::External(message) => ExecError::external(&message),
        ValueError::ExternalClass { class, message } => {
            let mut exec = ExecError::external(&message);
            exec.class_name = Some(class.to_owned());
            exec
        }
        ValueError::Interp { code, name1, name2 } => ExecError::new(code, None, &name1, &name2),
    }
}

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

/// The scripted filter of a run (`CustomScriptFilter` of
/// `mniNavFilterApplyClick`): `FilterScripted` with the script's `Filter`
/// function, called through the interpreter the run uses, so one
/// implementation serves `script.run` in one-shot, batch, serve and MCP
/// sessions.
struct RunScriptedFilter<'a> {
    script: &'a mut Script,
    state: &'a Rc<RefCell<ScriptRunState>>,
}

impl ScriptedFilter for RunScriptedFilter<'_> {
    fn enabled(&mut self) -> bool {
        self.state.borrow().variables.scripted
    }

    fn filter_record(&mut self, record: &ElementRef) -> Result<bool, ScriptedFilterError> {
        // `Script.FunctionExists('Filter')`: a script without the function
        // rejects every record (`CustomScriptFilter` is False).
        if !self.script.function_exists("Filter") {
            return Ok(false);
        }
        let value = self.state.borrow().element_value(record);
        match self.script.call_function("Filter", &[value]) {
            Ok(value) => match to_bool(&value) {
                Ok(accepted) => Ok(accepted),
                Err(_) => {
                    // `Boolean(V)` of a variant that does not convert: the
                    // raise happens in native code while the interpreter's
                    // frames are on the stack, so it records the position of
                    // the `Filter` call's last statement.
                    let message = format!(
                        "Could not convert variant of type ({}) into type (Boolean)",
                        value_type_name(&value)
                    );
                    let error = self.script.raise_external(VARIANT_CAST_CLASS, &message);
                    Err(scripted_error_of(self.script, &error))
                }
            },
            Err(error) => Err(scripted_error_of(self.script, &error)),
        }
    }
}

/// The raised error of a `Filter` call as the run's abort lines print it:
/// the class and the message of the exception, and the location the
/// interpreter recorded for it (`GetLastErrorLocation`).
fn scripted_error_of(script: &Script, error: &ExecError) -> ScriptedFilterError {
    ScriptedFilterError {
        location: script.last_error_location(),
        class: error.class_name().to_owned(),
        message: error.message().to_owned(),
    }
}

/// `wbHideStartTime` held up around the filter's application, whose lines
/// the GUI writes inside the call into the script (`ApplyScript`'s
/// `Inc(wbHideStartTime)` around `Initialize`): they carry no `[mm:ss]`
/// prefix. Restored on drop whichever path the application exits by.
struct HideStartTime(Rc<RefCell<ScriptRunState>>);

impl HideStartTime {
    fn new(state: &Rc<RefCell<ScriptRunState>>) -> HideStartTime {
        state.borrow_mut().hide_start_time += 1;
        HideStartTime(state.clone())
    }
}

impl Drop for HideStartTime {
    fn drop(&mut self) {
        self.0.borrow_mut().hide_start_time -= 1;
    }
}

/// `EVariantTypeCastError`: what `Boolean(V)` raises for a variant that does
/// not convert (Delphi's `VarCastError` family).
const VARIANT_CAST_CLASS: &str = "EVariantTypeCastError";

/// The `VarTypeAsText` name of a value, for the variant cast error's
/// message.
fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Str(_) => "UnicodeString",
        Value::Smallint(_) => "SmallInt",
        Value::Integer(_) => "Integer",
        Value::Int64(_) => "Int64",
        Value::Byte(_) => "Byte",
        Value::Double(_) | Value::Date(_) => "Double",
        Value::Bool(_) => "Boolean",
        Value::Object(_) => "IInterface",
        Value::Pointer(_) => "Pointer",
        _ => "Null",
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
/// passes. `FilterScripted` is cleared when the filter ends, whatever path
/// it exits by (the `finally` of `mniNavFilterApplyClick`).
///
/// `Ok(Some(error))` is the raised error of a scripted `Filter` call: the
/// walk aborted where upstream's exception leaves `mniNavFilterApplyClick`,
/// nothing else of the call that asked for the filter runs, and the caller
/// records it as the run's failure.
fn apply_pending_filters(
    session: &mut Session,
    state: &Rc<RefCell<ScriptRunState>>,
    script: &mut Script,
) -> Result<Option<ExecError>, CommandError> {
    loop {
        // The `borrow_mut()` of a `while let` condition would live for the
        // whole body, which borrows the state again.
        let Some(pending) = state.borrow_mut().pending_filter.take() else {
            break;
        };
        match pending {
            PendingFilter::Apply { request, at } => {
                // `ApplyScript`'s `Inc(wbHideStartTime)` around the call into
                // the script is still on while the filter runs upstream
                // (`ApplyFilter` is called from `Initialize`), so the
                // filter's lines carry no `[mm:ss]` prefix.
                let _hide = HideStartTime::new(state);
                // The relay's lines go first, so the block lands on the
                // message state the call recorded.
                state.borrow_mut().add_message("");
                let at = at.min(state.borrow().messages.len());
                let start = state.borrow().progress_line("Start: Applying Filter", false);
                let walk_from = state.borrow().messages.len();
                let outcome = crate::filter::filter_apply_scripted(
                    session,
                    *request,
                    Some(&mut RunScriptedFilter { script, state }),
                );
                // The `FilterScripted := False` of the filter's `finally`.
                state.borrow_mut().variables.scripted = false;
                // The lines the filter's own walk produced (the script's
                // `AddMessage` calls), which belong after the `Start:` line.
                let mut block: Vec<String> = start.into_iter().collect();
                block.extend(state.borrow_mut().messages.split_off(walk_from));
                match outcome {
                    Ok(applied) => {
                        let mut state = state.borrow_mut();
                        state.filter_applied = true;
                        state.filtered_nodes = Some(applied.nodes.iter().map(|node| node.get_element_id()).collect());
                        state.filtered_walk = Some(applied.walk.into_iter().collect());
                        // `ReInitTree` drops the old nodes and the filter
                        // classifies the new ones: what `ConflictAllForNode`
                        // reads afterwards.
                        state.host.node_conflicts = applied
                            .conflicts
                            .iter()
                            .map(|(id, all, this)| (*id, (*all, *this)))
                            .collect();
                        block.extend(applied.response.messages);
                        if let Some(line) = state.progress_line(
                            &format!(
                                "Done: Applying Filter, [Pass 1] Processed Records: {}, [Pass 2] Processed Records: {}, Remaining unfiltered nodes: {}",
                                applied.response.pass1, applied.response.pass2, applied.response.unfiltered
                            ),
                            false,
                        ) {
                            block.push(line);
                        }
                        // The messages of the statements that followed the
                        // `ApplyFilter` call stay, after the filter's lines.
                        for (offset, line) in block.into_iter().enumerate() {
                            state.messages.insert(at + offset, line);
                        }
                    }
                    Err(crate::filter::FilterApplyFailure::Command(error)) => return Err(error),
                    Err(crate::filter::FilterApplyFailure::Scripted(error)) => {
                        // `PerformLongAction`'s exception handler of the
                        // filter's own action (`Error during
                        // wbCurrentAction: <message>`), then the abort lines
                        // the run prints; the location comes from the
                        // script's own error state, as upstream reads
                        // `GetLastErrorLocation` when `ApplyScript`'s handler
                        // runs.
                        let mut state = state.borrow_mut();
                        if let Some(line) =
                            state.progress_line(&format!("Error during Applying Filter: {}", error.message), false)
                        {
                            block.push(line);
                        }
                        // The exception aborts the script at the
                        // `ApplyFilter` call, so the messages of the
                        // statements after it were never written.
                        state.messages.truncate(at);
                        for (offset, line) in block.into_iter().enumerate() {
                            state.messages.insert(at + offset, line);
                        }
                        let mut exec = ExecError::external(&error.message);
                        exec.class_name = Some(error.class);
                        return Ok(Some(exec));
                    }
                }
            }
            PendingFilter::Remove => {
                let _ = crate::filter::filter_remove(session, crate::filter::FilterRemoveRequest {})?;
                let mut state = state.borrow_mut();
                state.filter_applied = false;
                // `ReInitTree`: the tree holds every node again, unclassified.
                state.filtered_nodes = None;
                state.filtered_walk = None;
                state.host.node_conflicts.clear();
            }
        }
    }
    Ok(None)
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
        // `BuildAllRef` sets `wbStartTime := Now` before it builds: the
        // prefix clock of the run starts there.
        reset_progress_start();
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
        pending_interaction: None,
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
    // The VCL seam: the headless VCL object model (issue #26). Forms,
    // controls and events are real objects the script builds and reads
    // back; a dialog the script shows is answered from the recorded
    // `answers`, and one without an answer stops the run with
    // `pending_interaction` (the GUI phase renders the same model instead).
    let model = Rc::new(RefCell::new(crate::script_vcl::interactive::VclModel::new(
        name.clone(),
        request.answers.clone(),
    )));
    let vcl: Rc<RefCell<dyn crate::script_rtl::Vcl>> = model.clone();
    register_functions(&mut interpreter.adapter, &state, vcl);
    crate::script_units::register_units(&mut interpreter.adapter, &state);
    {
        // The host hooks run after the adapters: the published-property
        // surface of the VCL model (upstream's `GetValueRTTI`/`SetValueRTTI`,
        // which answer a control's published property whichever class the
        // registration tables carry it on), then the script variables
        // (`FilterByName := ...`) for a bare identifier.
        let state = state.clone();
        let setter_model = model.clone();
        interpreter.on_set_value = Some(Box::new(move |identifier, value, args| {
            if args.obj.is_some() {
                return setter_model
                    .borrow_mut()
                    .rtti_set(identifier, value, args)
                    .map_err(hook_error);
            }
            set_variable(&state, identifier, value)
        }));
        let getter_model = model.clone();
        interpreter.on_get_value = Some(Box::new(move |identifier, value, args| {
            if args.obj.is_none() {
                return Ok(false);
            }
            getter_model
                .borrow_mut()
                .rtti_get(identifier, value, args)
                .map_err(hook_error)
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
                    if let Some(error) = apply_pending_filters(session, &state, &mut script)? {
                        last_error = Some(error);
                        break 'run;
                    }
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
            clear_walk_positions();
            clear_walk_tree();
            if selection.live_walk() {
                // `ApplyScriptToSelection` over `vstNav`: each selected root
                // is walked from its last node back to itself, the next node
                // is read *before* `Process` runs, and the walk steps over
                // the navigation tree the GUI builds lazily: a node's
                // children are its container's elements at the moment the
                // walk first needs them and a child binds its element when
                // the walk reaches it, so an edit changes what the walk
                // reaches only where the tree has not looked yet (issue #84;
                // see [`WalkNode`]).
                enum Cursor {
                    /// A node of the navigation tree ([`WalkNode`]).
                    Node(usize),
                    /// A node of the filtered tree, with the element the
                    /// filter's walk bound to it.
                    Element(ElementRef),
                }
                'roots: for root in &order {
                    // With a filter on the tree the walk follows the nodes
                    // of the filtered tree with the elements they were
                    // bound to: a topic's responses keep the order the
                    // filter's walk bound them in, also when a check of the
                    // walk sorted the topic's group afterwards (parity #41,
                    // `filter-topic-bind.pas`). A root the filter's tree
                    // does not hold has nothing to walk.
                    let filtered = state
                        .borrow()
                        .filtered_walk
                        .as_ref()
                        .map(|walk| walk.get(&root.get_element_id()).cloned().unwrap_or_default());
                    let mut position = 0usize;
                    let mut root_node = None;
                    let mut cursor = match &filtered {
                        Some(nodes) => match nodes.first() {
                            Some(first) => Cursor::Element(first.clone()),
                            None => continue,
                        },
                        None => {
                            let node = walk_root(root);
                            root_node = Some(node);
                            Cursor::Node(last_node(node))
                        }
                    };
                    loop {
                        let (at_root, next) = if let Some(nodes) = &filtered {
                            (
                                position + 1 >= nodes.len(),
                                nodes.get(position + 1).cloned().map(Cursor::Element),
                            )
                        } else {
                            let node = match &cursor {
                                Cursor::Node(node) => *node,
                                Cursor::Element(_) => unreachable!("an unfiltered walk steps over tree nodes"),
                            };
                            (Some(node) == root_node, previous_node(node).map(Cursor::Node))
                        };
                        // A node of the filtered tree whose element an
                        // earlier `Process` removed is deleted with it
                        // (`_RemoveNode`): nothing to process. A tree node
                        // without an element (`NodeData.Element = nil`: a
                        // hidden child group, a slot the container no longer
                        // holds, or a `RemoveNode`) processes nothing either.
                        let removed = match (&filtered, &cursor) {
                            (Some(_), Cursor::Element(element)) => {
                                element.get_element_id() != root.get_element_id() && tree_parent(element).is_none()
                            }
                            _ => false,
                        };
                        let element = match &cursor {
                            Cursor::Node(node) => walk_element(*node),
                            Cursor::Element(element) => Some(element.clone()),
                        };
                        // `ApplyScriptToSelection` walks the live tree: a
                        // node a filter took out is not visited.
                        if let Some(node) = element
                            && !removed
                            && !state.borrow().is_filtered_out(&node)
                            && state.borrow().can_process(&node)
                        {
                            let params = {
                                let state = state.borrow();
                                [state.element_value(&node)]
                            };
                            // `ScriptProcessSingleElement`: without messages
                            // the walk unlocks the progress callback around
                            // the call.
                            if silent {
                                state.borrow_mut().progress_lock -= 1;
                            }
                            let result = call_script(&mut script, &state, "Process", &params);
                            if silent {
                                state.borrow_mut().progress_lock += 1;
                            }
                            match result {
                                Ok(value) => {
                                    if let Some(error) = apply_pending_filters(session, &state, &mut script)? {
                                        last_error = Some(error);
                                        break 'run;
                                    }
                                    let result = result_as_integer(&value);
                                    if result != 0 {
                                        // `ScriptProcessSingleElement <> 0`
                                        // exits the selection walk only
                                        // (`ApplyScriptToSelection`,
                                        // `xeMainForm.pas:8762`): `Finalize`
                                        // still runs, and the count does not
                                        // include this element.
                                        state.borrow_mut().progress(
                                            &format!("Script terminated itself, Result={result}").to_owned(),
                                            false,
                                        );
                                        self_terminated = Some(result);
                                        break 'roots;
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
                        if at_root {
                            break;
                        }
                        match next {
                            Some(next) => {
                                cursor = next;
                                position += 1;
                            }
                            // A root has no predecessor (`GetPrevious` of a
                            // file node is nil): the walk of this root is
                            // done.
                            None => break,
                        }
                    }
                }
                clear_walk_tree();
            } else {
                for element in &order {
                    // `ApplyScriptToSelection` walks the live tree: a node a
                    // filter took out is not visited.
                    if state.borrow().is_filtered_out(element) || !state.borrow().can_process(element) {
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
                            if let Some(error) = apply_pending_filters(session, &state, &mut script)? {
                                last_error = Some(error);
                                break 'run;
                            }
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
        }
        clear_walk_positions();
        clear_walk_tree();
        if script.function_exists("Finalize") && !script.function_is_empty("Finalize") {
            match call_script(&mut script, &state, "Finalize", &[]) {
                Ok(value) => {
                    if let Some(error) = apply_pending_filters(session, &state, &mut script)? {
                        last_error = Some(error);
                        break 'run;
                    }
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

    // A run that finished while a recorded answer is still unconsumed named
    // a dialog the run never showed; the oracle harness fails a run the same
    // way ("the oracle did not show the expected dialog"), so the port
    // reports it as a run failure and the two sides agree.
    if last_error.is_none()
        && let Some(answer) = model.borrow().leftover()
    {
        // A non-interpreter failure carrying the port's own text; the
        // distinct class names it in the log line.
        let mut error = ExecError::external(&format!(
            "the run never showed the dialog the recorded answer names ([{}] \"{}\")",
            answer.class, answer.title
        ));
        error.class_name = Some("EVclUnusedAnswers".to_owned());
        last_error = Some(error);
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
                // `PerformLongAction`'s `Now - wbLocalStartTime`: the action's
                // own clock, not the log prefix's.
                let elapsed = elapsed_text(state.action_started.elapsed());
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
    response.pending_interaction = model.borrow_mut().take_pending();

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
            // `NeedsRename := FileExists(wbDataPath + name)`: a loaded file
            // is the copy below the data path, so it exists and its save is
            // queued for renaming; a file the script made
            // (`AddNewFileName`) is not on disk yet and is saved under its
            // own name at once.
            if !Path::new(&format!("{data_path}{name}")).exists() {
                state.borrow_mut().progress(&format!("Saving: {name}"), false);
                let saved = crate::save::save_file(file, None, false, false)?;
                response.files.push(ScriptRunFile::from_save(file, &saved));
                continue;
            }
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
    crate::save::timestamp()
}

/// `script.run`: the command handler, over the GUI's selection.
fn script_run(session: &mut Session, request: ScriptRunRequest) -> Result<ScriptRunResponse, CommandError> {
    script_run_gated(session, request, &mut NavigationSelection)
}

#[cfg(test)]
mod tests {
    use super::is_silent;
    use super::{
        ElementImpl, ElementRef, ElementType, clear_walk_positions, clear_walk_tree, last_node, node_children,
        previous_node, signature_text, tree_parent, walk_element, walk_root,
    };

    /// The walk-churn fixture (`tests/fixtures/parity`) as the game master of
    /// an editable load order, the way the parity run loads it.
    fn walk_churn_file() -> ElementRef {
        crate::dump::setup_game_for_edit("sse").unwrap();
        crate::save::apply_edit_settings(xedit_core::interface::GameMode::gmSSE);
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/parity/walk-churn.esm"
        ))
        .unwrap();
        xedit_core::implementation::wb_file_from_bytes(
            "Skyrim.esm",
            i32::MAX,
            xedit_core::interface::types::FileStates::empty(),
            xedit_core::implementation::FileBytes::Owned(bytes),
        )
        .unwrap()
    }

    /// The walk over the lists of [`node_children`]: the last node of the
    /// previous sibling's subtree, or the parent.
    fn reference_walk(root: &ElementRef) -> Vec<usize> {
        fn last(node: &ElementRef) -> ElementRef {
            let mut node = node.clone();
            while let Some(child) = node_children(&node).last().cloned() {
                node = child;
            }
            node
        }
        let mut visited = Vec::new();
        let mut node = last(root);
        loop {
            visited.push(node.get_element_id());
            if node.get_element_id() == root.get_element_id() {
                break;
            }
            let parent = tree_parent(&node).expect("a node below the root has a parent");
            let siblings = node_children(&parent);
            let index = siblings
                .iter()
                .position(|sibling| sibling.get_element_id() == node.get_element_id())
                .expect("a node is a child of its parent");
            node = if index > 0 { last(&siblings[index - 1]) } else { parent };
        }
        visited
    }

    /// The elements the walk visits, in order, over the tree the walk builds.
    fn tree_walk(root: &ElementRef, mut process: impl FnMut(&ElementRef)) -> Vec<ElementRef> {
        clear_walk_tree();
        let root_node = walk_root(root);
        let mut node = last_node(root_node);
        let mut visited = Vec::new();
        loop {
            if let Some(element) = walk_element(node) {
                visited.push(element.clone());
                process(&element);
            }
            if node == root_node {
                break;
            }
            node = previous_node(node).expect("a node below the root has a predecessor");
        }
        clear_walk_tree();
        visited
    }

    /// The walk visits the nodes the child lists give, in their order, with
    /// the hidden child groups (which carry no element) stepped over: the
    /// walk-churn fixture holds worldspaces with their persistent and
    /// temporary cell children, a cell with references and a topic with
    /// responses.
    #[test]
    fn the_walk_steps_through_the_tree_the_child_lists_give() {
        let _guard = xedit_core::interface::globals::test_lock();
        xedit_core::implementation::clear_files_map();
        xedit_core::implementation::reset_load_order_slots();
        xedit_core::interface::clear_files();
        let root = walk_churn_file();
        let expected = reference_walk(&root);
        assert!(expected.len() > 20, "{}", expected.len());
        clear_walk_positions();
        let visited: Vec<usize> = tree_walk(&root, |_| {})
            .iter()
            .map(|element| element.get_element_id())
            .collect();
        clear_walk_positions();
        assert_eq!(visited, expected);
    }

    /// The walk of `Change persistent flag.pas` answered Yes over the
    /// walk-churn fixture, as the release visits it (`persistent-walk.pas`,
    /// parity #41): the references get the Persistent flag while the walk
    /// runs and move to the persistent children group of their cell.
    ///
    /// The interior cell's node enumerated its one child (the temporary
    /// group) before the moves, so the persistent group the moves create
    /// under it has no node and `REFR 0000080B` is visited once; the
    /// exterior cells the walk reaches later enumerate their children after
    /// the moves, so the moved references are found there. `CELL[P]`, the
    /// persistent cell the exterior moves create in the worldspace, has no
    /// node either: the worldspace's children were enumerated before.
    #[test]
    fn a_container_enumerates_its_children_once() {
        let _guard = xedit_core::interface::globals::test_lock();
        xedit_core::implementation::clear_files_map();
        xedit_core::implementation::reset_load_order_slots();
        xedit_core::interface::clear_files();
        let root = walk_churn_file();
        clear_walk_positions();
        clear_walk_tree();
        let root_node = walk_root(&root);
        let mut node = last_node(root_node);
        let mut visited = Vec::new();
        loop {
            if let Some(element) = walk_element(node)
                && element.get_element_type() == ElementType::etMainRecord
            {
                let signature = element.get_has_signature().map(signature_text).unwrap_or_default();
                let record = element.as_element_impl().and_then(ElementImpl::main_record_impl);
                let id = record
                    .as_ref()
                    .map(|record| record.mr_struct().form_id.to_cardinal())
                    .unwrap_or(0);
                visited.push(format!("{signature} {id:08X}"));
                if signature == "REFR"
                    && let Some(record) = record
                {
                    record.set_is_persistent(true);
                }
            }
            if node == root_node {
                break;
            }
            node = previous_node(node).expect("a node below the root has a predecessor");
        }
        clear_walk_tree();
        clear_walk_positions();
        // The release's walk (oracle log of `probes/persistent-walk.pas`,
        // 2026-10-17): `REFR 0000080B` and `0000080A` once each, then the
        // interior cell; `00000809` and `00000808` once each in the cell the
        // walk reaches after the move; no `CELL[P]` and no re-visit.
        let expected = [
            "INFO 0000080E",
            "INFO 0000080D",
            "INFO 0000080C",
            "DIAL 00000801",
            "REFR 0000080B",
            "REFR 0000080A",
            "CELL 00000807",
            "STAT 00000802",
            "WRLD 0000080F",
            "REFR 00000809",
            "REFR 00000808",
            "CELL 00000804",
            "REFR 00000806",
            "REFR 00000805",
            "CELL 00000803",
            "WRLD 00000800",
            "TES4 00000000",
        ];
        assert_eq!(visited, expected);
    }

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
