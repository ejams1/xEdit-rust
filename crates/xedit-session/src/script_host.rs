// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterHost.pas (the registrations
// the run state did not answer before issue #25), xEdit/xeMainForm.pas
// (AddNewFile, AddNewFileName, IsValidWindowsFileName, GetSanitizedFilename,
// AddRequiredMasters, IsPositionChanged, ConflictLevelForMainRecord's cache,
// JumpTo, FindNodeForElement, vstNavBeforeItemErase), Core/wbHelpers.pas
// (ExecuteCaptureConsoleOutput), Core/wbInterface.pas (_wbRecordDefMap,
// TwbFormID.FromVar/FromStr) and xEdit/xeFileSelectForm.pas (through the VCL
// model)

//! The rest of `xejviScriptAdapterHost.pas` (issue #25): the functions that
//! read or change the main form's state beyond the file list and the filter
//! of [`crate::script`].
//!
//! The main form's state is [`HostState`], held by the run state:
//!
//! * **The selection** of the navigation tree. `DoRunScript` selects every
//!   root node (the loaded files), so `wbSelectedFiles` and
//!   `wbSelectedFilesToFileNames` read the files; `JumpTo` focuses and
//!   selects the record's node alone, as `TMainRecordHistoryEntry.Show`
//!   does, which is what those two functions read afterwards. The walk of
//!   `Process` took its selection before `Initialize` ran, so a `JumpTo`
//!   does not change it.
//! * **The conflict status of the tree's nodes** (`NodeData.ConflictAll`,
//!   `ConflictThis`), which `ConflictAllForNode`/`ConflictThisForNode`
//!   read. A node starts unclassified (`caUnknown`); the filter classifies
//!   the nodes it walks (and keeps), `RemoveFilter` re-initialises the tree,
//!   and painting a main record classifies it. Headless nothing is painted
//!   except what `JumpTo` makes visible: the port classifies the record the
//!   jump focuses (the row the GUI is sure to paint; the other rows a window
//!   shows depend on its size and are not modelled).
//! * **The record conflict cache** (`TwbMainRecord.ConflictAll` and
//!   `ConflictThis`) that `ConflictLevelForMainRecord` fills and
//!   `IsPositionChanged` reads; an edit of the record or of a record of its
//!   override chain resets it (`ResetConflict`), which the port follows by
//!   the element generations of the chain.
//! * The `frmMain.Settings` object (`wbSettings`) and `_wbRecordDefMap`,
//!   each one object for the whole run.
//!
//! The dialogs of these functions (`AddNewFile`'s `InputQuery`, the
//! "file exists" message of `AddNewFileName`, the confirmation of
//! `AddRequiredElementMasters`) and the forms (`frmMain`,
//! `frmFileSelect`) are the VCL model's (issue #26): answered from the
//! recorded answers, or pending.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use xedit_analysis::conflict::{ConflictContext, ConflictOptions, ConflictStatus, files_in_load_order};
use xedit_core::implementation::{ElementImpl, FileImpl, MainRecordImpl};
use xedit_core::interface::element::MainRecordRef;
use xedit_core::interface::globals;
use xedit_core::interface::types::{ConflictAll, ConflictThis, ElementType, FileState, PascalEnum, Signature};
use xedit_core::interface::{Container, Element, ElementRef, FileRef, FormID, MainRecord};
use xedit_script::adapter::{Adapter, Args};
use xedit_script::error::{
    IE_DIRECT_INVALID_ARGUMENT, IE_NOT_ENOUGH_PARAMS, IE_TOO_MANY_PARAMS, IE_TYPE_MISMATCH, IE_UNIT_NOT_FOUND,
};
use xedit_script::strings::StringList as _;
use xedit_script::values::{ParamType, Value, ValueError, VarType, take_pending_variant_error};

use crate::script::{C_UNIT, ScriptRunState};
use crate::script_rtl::Vcl;
use crate::script_units::{element_at, list_at, strings_at};

/// The main form's state of a run (see the module documentation).
#[derive(Default)]
pub struct HostState {
    /// The selected nodes of the navigation tree; `None` is the selection
    /// `DoRunScript` makes (every root node: the files).
    pub selection: Option<Vec<ElementRef>>,
    /// `NodeData.ConflictAll`/`ConflictThis` of the classified nodes, by the
    /// identity of the node's element or container.
    pub node_conflicts: HashMap<usize, (ConflictAll, ConflictThis)>,
    /// `TwbMainRecord.ConflictAll`/`ConflictThis` of the records
    /// `ConflictLevelForMainRecord` classified, by the record's identity,
    /// with the element generations of its override chain at that time.
    pub record_conflicts: HashMap<usize, (i64, ConflictStatus)>,
    /// `frmMain.Settings` (`wbSettings`).
    pub settings: Option<Value>,
    /// `_wbRecordDefMap` (`wbRecordDefMap`).
    pub record_def_map: Option<Value>,
}

//=== Helpers ==================================================================

/// `IInterface(Args.Values[i])` with the cast error raised at once: `nil`
/// and objects that are no element read as `None`, a number or a string is
/// the variant cast error upstream raises inside the handler.
fn interface_at(args: &Args, index: usize) -> Result<Option<ElementRef>, ValueError> {
    let element = element_at(args, index);
    take_pending_variant_error()?;
    Ok(element)
}

/// `Supports(IInterface(Args.Values[i]), IwbMainRecord, ...)`.
fn main_record_arg(args: &Args, index: usize) -> Result<Option<MainRecordRef>, ValueError> {
    Ok(interface_at(args, index)?.and_then(|element| element.into_main_record()))
}

fn invalid_argument() -> ValueError {
    ValueError::interp(IE_DIRECT_INVALID_ARGUMENT)
}

/// The implementation of a main record.
fn record_impl(record: &ElementRef) -> Option<Arc<MainRecordImpl>> {
    record.as_element_impl().and_then(ElementImpl::main_record_impl)
}

/// The implementation of a file element.
fn file_impl(file: &ElementRef) -> Option<Arc<FileImpl>> {
    file.as_element_impl().and_then(ElementImpl::file_impl)
}

/// `TwbFile.GetFileName`: the file's name, the executable's for the
/// hardcoded file.
pub(crate) fn file_name_of(file: &FileRef) -> String {
    if file.get_file_states().contains(FileState::fsIsHardcoded) {
        globals::game_exe_name()
    } else {
        file.get_name()
    }
}

/// The loaded files in the order of upstream's `Files`, as the conflict and
/// LOD code takes them.
fn run_files() -> Vec<Arc<FileImpl>> {
    files_in_load_order(
        xedit_core::interface::files()
            .into_iter()
            .filter_map(|file| file.as_element_impl().and_then(ElementImpl::file_impl))
            .collect(),
    )
}

/// A host exception with the RTL's `Exception` class.
fn exception(message: impl Into<String>) -> ValueError {
    ValueError::external_class("Exception", message.into())
}

/// `StrToInt64('$' + s)`'s `EConvertError`.
fn invalid_integer(text: &str) -> ValueError {
    ValueError::external_class("EConvertError", format!("'{text}' is not a valid integer value"))
}

/// `TwbFormID.FromStr`: `StrToInt64('$' + s)` (spaces dropped under
/// `wbPrettyFormID`), the low 32 bits.
fn form_id_from_str(text: &str) -> Result<FormID, ValueError> {
    FormID::from_str(text).ok_or_else(|| {
        let text = if globals::pretty_form_id() {
            text.replace(' ', "")
        } else {
            text.to_owned()
        };
        invalid_integer(&format!("${text}"))
    })
}

/// `TwbFormID.FromVar`: an ordinal value as the FormID (a negative
/// `Integer` as its `Cardinal`, a value beyond `High(Cardinal)` or below -1
/// leaves the FormID 0), a pointer as its address, anything else through
/// its text as hexadecimal digits.
fn form_id_from_var(value: &Value) -> Result<FormID, ValueError> {
    if value.is_ordinal() {
        let mut int64 = xedit_script::values::to_i64(value)?;
        if int64 < -1 && int64 >= i64::from(i32::MIN) {
            int64 = i64::from(int64 as i32 as u32);
        }
        if int64 > i64::from(u32::MAX) {
            int64 = -1;
        }
        return Ok(if int64 >= 0 {
            FormID::from_cardinal(int64 as u32)
        } else {
            FormID::null()
        });
    }
    if let Value::Pointer(address) = value {
        return Ok(FormID::from_cardinal(*address as u32));
    }
    let text = String::from_utf8_lossy(&xedit_script::values::to_str(value)?).into_owned();
    let parsed = xedit_core::interface::misc::str_to_int64(&format!("${text}"))
        .map_err(|_| invalid_integer(&format!("${text}")))?;
    Ok(FormID::from_cardinal(parsed as u32))
}

/// `VarIsNumeric`.
fn is_numeric(value: &Value) -> bool {
    value.is_ordinal() || matches!(value, Value::Double(_) | Value::Date(_))
}

/// The sum of the element generations of a record's override chain: a
/// change of any of them resets the cached conflict status
/// (`TwbMainRecord.ResetConflict` resets the master and every override).
fn chain_generation(record: &Arc<MainRecordImpl>) -> i64 {
    let master = record.master().unwrap_or_else(|| record.clone());
    let mut total = i64::from(master.element_base().element_generation());
    for over in master.overrides() {
        total += i64::from(over.element_base().element_generation());
    }
    total
}

/// `TfrmMain.ConflictLevelForMainRecord`, recording the statuses it finds in
/// the run's record cache (`TwbMainRecord.ConflictAll := ...`).
pub(crate) fn conflict_level_for_main_record(
    state: &RefCell<ScriptRunState>,
    record: &Arc<MainRecordImpl>,
) -> ConflictStatus {
    let files = run_files();
    let options = ConflictOptions::default();
    let mut context = ConflictContext::new(&options, &files);
    let status = context.conflict_level_for_main_record(record);
    let master = record.master().unwrap_or_else(|| record.clone());
    let mut chain = vec![master.clone()];
    chain.extend(master.overrides());
    let generation = chain_generation(record);
    let mut state = state.borrow_mut();
    for member in chain {
        let member_status = context.status(&member);
        state
            .host
            .record_conflicts
            .insert(member_identity(&member), (generation, member_status));
    }
    status
}

fn member_identity(record: &Arc<MainRecordImpl>) -> usize {
    Arc::as_ptr(record) as *const () as usize
}

/// `TwbMainRecord.ConflictThis` as cached: the status
/// `ConflictLevelForMainRecord` left, `ctUnknown` when it never ran or an
/// edit of the chain reset it.
fn cached_conflict_this(state: &RefCell<ScriptRunState>, record: &Arc<MainRecordImpl>) -> ConflictThis {
    let generation = chain_generation(record);
    match state.borrow().host.record_conflicts.get(&member_identity(record)) {
        Some((cached, status)) if *cached == generation => status.this,
        _ => ConflictThis::ctUnknown,
    }
}

/// Port of `IsPositionChanged` (`xeMainForm.pas:13381`): an override whose
/// `DATA` (the position) differs from its master's, unless its cached
/// conflict status says it is the master or identical to it.
fn is_position_changed(state: &RefCell<ScriptRunState>, record: &Arc<MainRecordImpl>) -> bool {
    if matches!(
        cached_conflict_this(state, record),
        ConflictThis::ctMaster | ConflictThis::ctIdenticalToMaster
    ) {
        return false;
    }
    if record.get_is_master() || record.get_is_deleted() {
        return false;
    }
    let Some(master) = record.master() else {
        return false;
    };
    let data = Signature::new(b"DATA");
    match (
        master.get_record_by_signature(data),
        record.get_record_by_signature(data),
    ) {
        (None, None) => false,
        (Some(_), None) | (None, Some(_)) => true,
        (Some(master_pos), Some(this_pos)) => master_pos.get_sort_key(true) != this_pos.get_sort_key(true),
    }
}

/// The node of the navigation tree an element has (`FindNodeForElement`),
/// as the identity its conflict status is kept under: files, groups and
/// main records have nodes, a child group shows as its record's node, and
/// a filter keeps the nodes it did not take out. `None` when the element
/// has no node.
fn node_of(state: &ScriptRunState, element: &ElementRef) -> Option<usize> {
    let kind = element.get_element_type();
    if !matches!(
        kind,
        ElementType::etFile | ElementType::etGroupRecord | ElementType::etMainRecord
    ) {
        return None;
    }
    if state.is_filtered_out(element) && !state.host.node_conflicts.contains_key(&element.get_element_id()) {
        return None;
    }
    Some(element.get_element_id())
}

/// The selected nodes (`vstNav.GetSortedSelection(True)`).
fn selected_nodes(state: &ScriptRunState) -> Vec<ElementRef> {
    match &state.host.selection {
        Some(selection) => selection.clone(),
        None => state.files().to_vec(),
    }
}

/// `IsValidWindowsFileName`.
fn is_valid_windows_file_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| "\\/*?:\"<>|".contains(c))
}

/// `GetSanitizedFilename`: trimmed, without a plugin extension.
fn sanitized_file_name(name: &str) -> String {
    let name = name.trim();
    let extension = xedit_core::implementation::masters::extract_file_ext(name);
    if [".esp", ".esm", ".esl"]
        .iter()
        .any(|ext| extension.eq_ignore_ascii_case(ext))
    {
        name[..name.len() - extension.len()].to_owned()
    } else {
        name.to_owned()
    }
}

/// Asks the VCL model one of its dialog functions and returns the value.
fn vcl_function(vcl: &Rc<RefCell<dyn Vcl>>, function: &str, values: Vec<Value>) -> Result<(Value, Args), ValueError> {
    let mut args = Args::new();
    args.identifier = function.to_owned();
    args.values = values;
    let mut value = Value::Empty;
    vcl.borrow_mut().function("", function, &mut value, &mut args)?;
    Ok((value, args))
}

/// `TfrmMain.AddNewFileName(aFileName, aIsLight, aIsMedium)`: the new
/// file in the data folder, after the last of `Files`; `None` when a file
/// of that name exists in the data folder (the message the GUI shows).
fn add_new_file_name(
    state: &Rc<RefCell<ScriptRunState>>,
    vcl: &Rc<RefCell<dyn Vcl>>,
    name: &str,
    light: bool,
    medium: bool,
) -> Result<Option<ElementRef>, ValueError> {
    if !is_valid_windows_file_name(name) {
        return Err(exception(format!(
            "The specified filename:\r\n\r\n{name}\r\n\r\nContains one or more invalid characters:\r\n\r\n\\/*?:\"<>|"
        )));
    }
    let path = format!("{}{name}", globals::data_path());
    if std::path::Path::new(&path).exists() {
        vcl_function(
            vcl,
            "ShowMessage",
            vec![Value::Str(b"A file of that name exists already.".to_vec())],
        )?;
        return Ok(None);
    }
    // `LoadOrder := Succ(Files[High(Files)].LoadOrder)`: after the last of
    // the list, whatever the others hold.
    let load_order = state
        .borrow()
        .files()
        .last()
        .and_then(|file| file.get_file())
        .map_or(0, |file| file.get_load_order() + 1);
    // The release's file name is the full path with the separators
    // collapsed (the data path itself ends in a doubled one, as the GUI's
    // `Using ... Data Path:` line shows, while the "exists already" message
    // and the saved file name have one; `GetFullPathName`'s form).
    let path = std::path::absolute(&path)
        .map(|full| full.display().to_string())
        .unwrap_or(path);
    let file =
        xedit_core::implementation::new_file::wb_new_file(&path, load_order, light, medium).map_err(exception)?;
    let element = file
        .self_element_ref()
        .ok_or_else(|| exception("the new file has no element"))?;
    state.borrow_mut().push_file(element.clone());
    Ok(Some(element))
}

/// `TfrmMain.AddNewFile(aFile, aIsLight, aIsMedium)`: the name from the
/// `InputQuery`, sanitized, `.esl` for a light module, else `.esp`, and the
/// game master as master (`wbAlwaysLoadGameMaster`).
fn add_new_file(
    state: &Rc<RefCell<ScriptRunState>>,
    vcl: &Rc<RefCell<dyn Vcl>>,
    light: bool,
    medium: bool,
) -> Result<Option<ElementRef>, ValueError> {
    let (answered, args) = vcl_function(
        vcl,
        "InputQuery",
        vec![
            Value::Str(b"New Module File".to_vec()),
            Value::Str(b"Filename without extension:".to_vec()),
            Value::Str(Vec::new()),
        ],
    )?;
    if !matches!(answered, Value::Bool(true)) {
        return Ok(None);
    }
    let typed = String::from_utf8_lossy(&xedit_script::values::to_str(&args.values[2])?).into_owned();
    let base = sanitized_file_name(&typed);
    if base.is_empty() {
        return Ok(None);
    }
    let name = format!("{base}{}", if light { ".esl" } else { ".esp" });
    let file = add_new_file_name(state, vcl, &name, light, medium)?;
    if let Some(file) = &file
        && globals::always_load_game_master()
        && let Some(file) = file_impl(file)
    {
        file.add_masters_if_missing(&[globals::game_master_esm()], true, false)
            .map_err(exception)?;
    }
    Ok(file)
}

/// `TfrmMain.AddRequiredMasters(aSourceElement, aTargetFile, aAsNew,
/// aSilent)`: the masters a copy of the element needs that the target
/// lacks (with their own masters under `wbEnforceAllMasters`), refused when
/// one loads after the target, confirmed unless silent, added in load
/// order.
fn add_required_masters(
    vcl: &Rc<RefCell<dyn Vcl>>,
    element: &ElementRef,
    target: &Arc<FileImpl>,
    as_new: bool,
    silent: bool,
) -> Result<bool, ValueError> {
    use xedit_core::implementation::copy::required_masters;
    let mut masters = Vec::new();
    for file in required_masters(element, as_new) {
        if globals::enforce_all_masters() {
            masters.extend(file.all_masters());
        }
        masters.push(file);
    }
    // The sorted, duplicate-free name list (`TStringList` with `Sorted` and
    // `dupIgnore`), less the target's masters and the target itself.
    let mut names: Vec<Arc<FileImpl>> = Vec::new();
    for file in masters {
        if !names
            .iter()
            .any(|known| xedit_io::encoding::ansi_compare_text(&known.get_name(), &file.get_name()).is_eq())
        {
            names.push(file);
        }
    }
    names.sort_by(|a, b| xedit_io::encoding::ansi_compare_text(&a.get_name(), &b.get_name()));
    let own: Vec<String> = target.masters().iter().map(|master| master.get_name()).collect();
    names.retain(|file| {
        !own.iter().any(|name| name.eq_ignore_ascii_case(&file.get_name()))
            && !file.get_name().eq_ignore_ascii_case(&target.get_name())
    });
    if names.is_empty() {
        return Ok(true);
    }
    for file in &names {
        if file.load_order() >= target.load_order() {
            return Err(exception(format!(
                "The required master \"{}\" can not be added to \"{}\" as it has a higher load order",
                file.get_name(),
                target.get_name()
            )));
        }
    }
    let mut result = silent;
    if !result {
        let text: String = names.iter().map(|file| format!("{}\r\n", file.get_name())).collect();
        let message = format!(
            "To continue the following files need to be added to \"{}\" as masters:\r\r{text}\rDo you want to continue?",
            target.get_name()
        );
        // `MessageDlg(..., mtConfirmation, [mbYes, mbNo], 0) = mrYes`.
        let (answer, _) = vcl_function(
            vcl,
            "MessageDlg",
            vec![
                Value::Str(message.into_bytes()),
                Value::Integer(3),
                Value::Set(0b11),
                Value::Integer(0),
            ],
        )?;
        result = matches!(answer, Value::Integer(6));
    }
    // `CustomSort(CompareLoadOrder)`.
    names.sort_by_key(|file| file.load_order());
    if result {
        let names: Vec<String> = names.iter().map(|file| file.get_name()).collect();
        target.add_masters_if_missing(&names, true, false).map_err(exception)?;
    }
    Ok(result)
}

/// The `LodEnv` of a script's LOD calls: `frmMain.Settings` is the
/// `wbSettings` object when the script has read it (its unsaved changes
/// included, as upstream passes the same `TMemIniFile`), else the settings
/// file; the Fallout 4 and Starfield overrides are the ones the GUI's
/// `wbLODDefaults` sets (as `lodgen.rs` applies them).
pub(crate) fn lod_env(state: &RefCell<ScriptRunState>) -> xedit_assets::lod::LodEnv {
    use xedit_assets::sniff::processor::MemIniFile;
    let in_memory = state
        .borrow()
        .host
        .settings
        .as_ref()
        .and_then(crate::script_vcl::misc_vcl::ini_text_of);
    let settings = match in_memory {
        Some(text) => MemIniFile::from_text(&text),
        None => {
            let ini = crate::modgroups::settings_file();
            if ini.is_file() {
                MemIniFile::load(&ini)
            } else {
                MemIniFile::default()
            }
        }
    };
    let mut defaults = xedit_assets::lod::LodDefaults::initial();
    if globals::is_fallout4() || globals::is_starfield() {
        defaults.atlas_width = 4096;
        defaults.atlas_height = 4096;
        defaults.uv_range = 1.1;
        defaults.atlas_diffuse_format = xedit_assets::imaging::ImageFormat::Dxt5;
        defaults.atlas_normal_format = xedit_assets::imaging::ImageFormat::Ati2n;
    }
    xedit_assets::lod::LodEnv {
        data_path: globals::data_path(),
        output_path: globals::output_path(),
        scripts_path: globals::scripts_path(),
        temp_path: globals::temp_path(),
        settings,
        defaults,
    }
}

/// The object of a `TStrings` argument as the LOD list (the lines with the
/// integer their object holds; a nil or other object counts as 0).
pub(crate) fn lod_list(value: &Value) -> Option<xedit_assets::lod::StringList> {
    let object = crate::script_rtl::classes::strings_object(value)?;
    let borrowed = object.borrow();
    let list = borrowed.string_list()?;
    let mut result = xedit_assets::lod::StringList::new();
    result.sorted = list.sorted();
    result.ignore_duplicates = matches!(list.duplicates(), xedit_script::strings::Duplicates::dupIgnore);
    for (line, object) in list.lines().iter().zip(list.objects()) {
        let flag = match object {
            Value::Integer(flag) => u64::try_from(*flag).unwrap_or(0),
            Value::Int64(flag) => u64::try_from(*flag).unwrap_or(0),
            Value::Pointer(flag) => *flag as u64,
            _ => 0,
        };
        result.items.push((xedit_io::encoding::ansi_string(line), flag));
    }
    Some(result)
}

//=== ExecuteCaptureConsoleOutput ============================================

/// Port of `ExecuteCaptureConsoleOutput` (`wbHelpers.pas:940`): the command
/// line runs hidden with its standard output and error on one pipe, polled
/// every 100 ms; each read is converted from the OEM code page, trimmed and
/// logged (`wbProgressCallback`); the result is the exit code. A failed
/// `CreateProcess` raises `RaiseLastOSError`'s `EOSError`.
/// UPSTREAM-BUG: upstream copies the command line into a `MAX_PATH + 1`
/// character buffer with `StrPCopy`, which overruns the stack for a longer
/// line; the port passes the whole line.
#[cfg(windows)]
fn execute_capture_console_output(state: &RefCell<ScriptRunState>, command_line: &str) -> Result<u32, ValueError> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, TRUE, WAIT_FAILED, WAIT_TIMEOUT};
    use windows_sys::Win32::Globalization::{CP_OEMCP, MultiByteToWideChar};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::ReadFile;
    use windows_sys::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, NORMAL_PRIORITY_CLASS, PROCESS_INFORMATION, STARTF_USESHOWWINDOW,
        STARTF_USESTDHANDLES, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    const READ_BUFFER: usize = 4096;
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: TRUE,
    };
    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();
    // SAFETY: the handles are out parameters; the attributes live for the
    // call.
    if unsafe { CreatePipe(&mut read, &mut write, &security, 0) } == 0 {
        // Upstream does nothing without a pipe (the result is undefined; 0
        // here).
        return Ok(0);
    }
    let mut exit_code: u32 = 0;
    let outcome = (|| -> Result<u32, ValueError> {
        // SAFETY: a zeroed `STARTUPINFOW` is the `FillChar(..., #0)` upstream
        // does before it sets the fields.
        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        startup.hStdInput = read;
        startup.hStdOutput = write;
        startup.hStdError = write;
        startup.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
        startup.wShowWindow = SW_HIDE as u16;
        let mut line: Vec<u16> = command_line.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: a zeroed `PROCESS_INFORMATION` is an out parameter.
        let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the command line is a writable NUL-terminated buffer, as
        // `CreateProcessW` requires; the attributes, the startup info and
        // the process information live for the call.
        let created = unsafe {
            CreateProcessW(
                std::ptr::null(),
                line.as_mut_ptr(),
                &security,
                &security,
                TRUE,
                NORMAL_PRIORITY_CLASS,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut process,
            )
        };
        if created == 0 {
            let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32;
            return Err(ValueError::external_class(
                "EOSError",
                crate::script_rtl::os_error_message_code(code),
            ));
        }
        let mut buffer = vec![0u8; READ_BUFFER + 1];
        loop {
            // SAFETY: the process handle is valid until it is closed below.
            let running = unsafe { WaitForSingleObject(process.hProcess, 100) };
            if globals::force_terminate() {
                // SAFETY: as above.
                unsafe {
                    if TerminateProcess(process.hProcess, 1) != 0
                        && WaitForSingleObject(process.hProcess, 1000) == WAIT_FAILED
                    {
                        exit_code = std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32;
                    }
                }
                state.borrow_mut().progress("Interrupted by user!", false);
                // SAFETY: the handles came from `CreateProcessW`.
                unsafe {
                    CloseHandle(process.hProcess);
                    CloseHandle(process.hThread);
                }
                return Ok(exit_code);
            }
            let mut available: u32 = 0;
            // SAFETY: the pipe handle is valid; only the available count is
            // asked for.
            if unsafe {
                PeekNamedPipe(
                    read,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            } != 0
                && available > 0
            {
                loop {
                    let mut count: u32 = 0;
                    // SAFETY: the buffer holds `READ_BUFFER + 1` bytes and at
                    // most `READ_BUFFER` are read.
                    unsafe {
                        ReadFile(
                            read,
                            buffer.as_mut_ptr().cast(),
                            READ_BUFFER as u32,
                            &mut count,
                            std::ptr::null_mut(),
                        );
                    }
                    let bytes = &buffer[..count as usize];
                    // `string(OemString(pBuffer))`: the OEM code page, up to
                    // the first NUL.
                    let bytes = bytes.split(|byte| *byte == 0).next().unwrap_or(&[]);
                    let text = if bytes.is_empty() {
                        String::new()
                    } else {
                        // SAFETY: the sizes are the buffers' own.
                        let length = unsafe {
                            MultiByteToWideChar(
                                CP_OEMCP,
                                0,
                                bytes.as_ptr(),
                                bytes.len() as i32,
                                std::ptr::null_mut(),
                                0,
                            )
                        };
                        let mut wide = vec![0u16; length.max(0) as usize];
                        // SAFETY: `wide` holds the length the first call
                        // reported.
                        unsafe {
                            MultiByteToWideChar(
                                CP_OEMCP,
                                0,
                                bytes.as_ptr(),
                                bytes.len() as i32,
                                wide.as_mut_ptr(),
                                length,
                            );
                        }
                        String::from_utf16_lossy(&wide)
                    };
                    // `Trim`: control characters and spaces at both ends.
                    let trimmed = text.trim_matches(|c: char| c <= ' ');
                    if !trimmed.is_empty() {
                        state.borrow_mut().progress(trimmed, false);
                    }
                    if (count as usize) < READ_BUFFER {
                        break;
                    }
                }
            }
            if running != WAIT_TIMEOUT {
                break;
            }
        }
        // SAFETY: the handles came from `CreateProcessW`.
        unsafe {
            GetExitCodeProcess(process.hProcess, &mut exit_code);
            CloseHandle(process.hProcess);
            CloseHandle(process.hThread);
        }
        Ok(exit_code)
    })();
    // SAFETY: the pipe's two handles, closed once.
    unsafe {
        CloseHandle(read);
        CloseHandle(write);
    }
    outcome
}

#[cfg(not(windows))]
fn execute_capture_console_output(_state: &RefCell<ScriptRunState>, _command_line: &str) -> Result<u32, ValueError> {
    Err(exception("ExecuteCaptureConsoleOutput needs Windows"))
}

//=== The registrations ======================================================

/// Registers the issue #25 functions of `xejviScriptAdapterHost.pas`.
pub(crate) fn register(adapter: &mut Adapter, state: &Rc<RefCell<ScriptRunState>>, vcl: &Rc<RefCell<dyn Vcl>>) {
    fn add(adapter: &mut Adapter, name: &str, count: i32, func: xedit_script::adapter::AdapterGetFn) {
        let params = if count < 0 {
            Vec::new()
        } else {
            (0..count).map(|_| ParamType::value(VarType::Empty)).collect()
        };
        adapter.add_function(C_UNIT, name, func, count, params, VarType::Empty);
    }

    //=== Zero-argument globals ===============================================

    for (name, getter) in [
        ("wbLoadBSAs", globals::load_bsas as fn() -> bool),
        ("wbSimpleRecords", globals::simple_records),
        ("wbDecodeTextureHashes", globals::decode_texture_hashes),
        ("wbTrackAllEditorID", globals::track_all_editor_id),
    ] {
        add(
            adapter,
            name,
            0,
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                *value = Value::Bool(getter());
                Ok(())
            }),
        );
    }
    add(
        adapter,
        "wbSettingsFileName",
        0,
        Rc::new(|value: &mut Value, _args: &mut Args| {
            *value = Value::Str(crate::modgroups::settings_file().display().to_string().into_bytes());
            Ok(())
        }),
    );
    {
        // `O2V(frmMain.Settings)`: one `TMemIniFile` over the settings file
        // for the whole run (the main form reads it at its start).
        let state = state.clone();
        let class = adapter.find_class(b"TMemIniFile");
        add(
            adapter,
            "wbSettings",
            0,
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                let existing = state.borrow().host.settings.clone();
                let settings = match existing {
                    Some(settings) => settings,
                    None => {
                        let Some(class) = &class else {
                            return Err(exception("the class TMemIniFile is not registered"));
                        };
                        let path = crate::modgroups::settings_file().display().to_string();
                        let settings = crate::script_vcl::misc_vcl::mem_ini_file_value(class.clone(), path);
                        state.borrow_mut().host.settings = Some(settings.clone());
                        settings
                    }
                };
                *value = settings;
                Ok(())
            }),
        );
    }
    {
        // `O2V(_wbRecordDefMap)`: a `TwbFastStringList` (a `TStringList`
        // comparing with the locale, without case) of the signature of every
        // record definition, the definition as its object, sorted once
        // filled.
        let state = state.clone();
        let class = adapter.find_class(b"TStringList");
        add(
            adapter,
            "wbRecordDefMap",
            0,
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                let existing = state.borrow().host.record_def_map.clone();
                let map = match existing {
                    Some(map) => map,
                    None => {
                        let Some(class) = &class else {
                            return Err(exception("the class TStringList is not registered"));
                        };
                        let mut list = crate::script_strings::StringListObject::new(class.clone());
                        for (signature, def) in xedit_core::interface::constructors::record_defs() {
                            list.add_object(
                                signature.0.to_vec(),
                                // The definition's address stands for the
                                // `Pointer(rdeDef)` (`Assigned` is true).
                                Value::Pointer(Arc::as_ptr(&def) as *const () as usize),
                            );
                        }
                        list.set_sorted(true);
                        let map = Value::Object(Rc::new(RefCell::new(list)));
                        state.borrow_mut().host.record_def_map = Some(map.clone());
                        map
                    }
                };
                *value = map;
                Ok(())
            }),
        );
    }

    //=== The forms ===========================================================

    for (name, function) in [("frmMain", "frmMain"), ("frmFileSelect", "frmFileSelect")] {
        let vcl = vcl.clone();
        add(
            adapter,
            name,
            0,
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                let (result, _) = vcl_function(&vcl, function, Vec::new())?;
                *value = result;
                Ok(())
            }),
        );
    }

    //=== The console ========================================================

    {
        let state = state.clone();
        add(
            adapter,
            "ExecuteCaptureConsoleOutput",
            1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let command = String::from_utf8_lossy(&xedit_script::values::to_str(&args.values[0])?).into_owned();
                let code = execute_capture_console_output(&state, &command)?;
                // A `Cardinal`: the port carries it as an `Int64` (the
                // documented slot divergence; upstream's is `varLongWord`).
                *value = Value::Int64(i64::from(code));
                Ok(())
            }),
        );
    }

    //=== The files ==========================================================

    {
        let state = state.clone();
        add(
            adapter,
            "FileByLoadOrderFileID",
            1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let Value::Str(text) = &args.values[0] else {
                    return Err(invalid_argument());
                };
                let text = String::from_utf8_lossy(text).into_owned();
                let state = state.borrow();
                for file in state.files() {
                    if let Some(loaded) = file.get_file()
                        && loaded.get_load_order_file_id().to_string() == text
                    {
                        *value = state.element_value(file);
                        break;
                    }
                }
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        add(
            adapter,
            "FileByLoadOrder",
            1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let state = state.borrow();
                let arg = &args.values[0];
                // `VarIsNumeric(...) and (Args.Values[0] < Length(Files))`.
                if !is_numeric(arg) || xedit_script::values::to_f64(arg)? >= state.files().len() as f64 {
                    return Err(invalid_argument());
                }
                let load_order = xedit_script::values::to_i32(arg)?;
                for file in state.files() {
                    if file
                        .get_file()
                        .is_some_and(|loaded| loaded.get_load_order() == load_order)
                    {
                        *value = state.element_value(file);
                        break;
                    }
                }
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        add(
            adapter,
            "RecordFromFileByFormID",
            -1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                match args.values.len() {
                    0 | 1 => return Err(ValueError::interp(IE_NOT_ENOUGH_PARAMS)),
                    2 => {}
                    _ => return Err(ValueError::interp(IE_TOO_MANY_PARAMS)),
                }
                let file: FileRef = if let Value::Str(name) = &args.values[0] {
                    let name = String::from_utf8_lossy(name).into_owned();
                    let found = state
                        .borrow()
                        .files()
                        .iter()
                        .filter_map(|file| file.get_file())
                        .find(|file| file_name_of(file).eq_ignore_ascii_case(&name));
                    match found {
                        Some(file) => file,
                        None => return Err(ValueError::interp_n(IE_UNIT_NOT_FOUND, name)),
                    }
                } else {
                    match interface_at(args, 0)?
                        .filter(|element| element.get_element_type() == ElementType::etFile)
                        .and_then(|element| element.get_file())
                    {
                        Some(file) => file,
                        None => return Err(ValueError::interp(IE_TYPE_MISMATCH)),
                    }
                };
                let mut form_id = match &args.values[1] {
                    Value::Str(text) => form_id_from_str(&String::from_utf8_lossy(text))?,
                    other if is_numeric(other) => form_id_from_var(other)?,
                    _ => FormID::null(),
                };
                let file_id = file.get_load_order_file_id();
                let mask = if file_id.is_light_slot() {
                    0xFFF
                } else if file_id.is_medium_slot() {
                    0xFFFF
                } else {
                    0xFF_FFFF
                };
                let object_id = form_id.object_id() & mask;
                form_id
                    .set_object_id(object_id)
                    .map_err(|_| ValueError::external_class("ERangeError", "Range check error".to_owned()))?;
                form_id.set_file_id(file_id);
                if let Ok(Some(record)) = file.get_record_by_form_id(form_id, true, true) {
                    *value = state.borrow().element_value(&(record as ElementRef));
                }
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        add(
            adapter,
            "RecordByHexFormID",
            1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let Value::Str(text) = &args.values[0] else {
                    return Err(invalid_argument());
                };
                *value = Value::Null;
                let form_id = form_id_from_str(&String::from_utf8_lossy(text))?;
                let state = state.borrow();
                for file in state.files() {
                    let Some(loaded) = file.get_file() else {
                        continue;
                    };
                    if loaded.get_load_order_file_id() == form_id.file_id() {
                        // `Value := Files[i].RecordByFormID[...]`: the record,
                        // or the nil interface (no longer `Null`, so a call
                        // that takes it reaches its own refusal, probe
                        // `host-dialogs` over the probe master).
                        *value = match loaded.get_record_by_form_id(form_id, true, true) {
                            Ok(Some(record)) => state.element_value(&(record as ElementRef)),
                            _ => Value::Empty,
                        };
                        break;
                    }
                }
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        add(
            adapter,
            "IsPositionChanged",
            1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let Some(record) = main_record_arg(args, 0)?.and_then(|record| record_impl(&(record as ElementRef)))
                else {
                    return Err(invalid_argument());
                };
                *value = Value::Bool(is_position_changed(&state, &record));
                Ok(())
            }),
        );
    }

    //=== New files and masters ===============================================

    {
        let state = state.clone();
        let vcl = vcl.clone();
        add(
            adapter,
            "AddNewFile",
            -1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let flag = |index: usize| -> Result<bool, ValueError> {
                    args.values.get(index).map_or(Ok(false), xedit_script::values::to_bool)
                };
                let file = match args.values.len() {
                    0 => add_new_file(&state, &vcl, false, false)?,
                    1 => add_new_file(&state, &vcl, flag(0)?, false)?,
                    2 => add_new_file(&state, &vcl, flag(0)?, flag(1)?)?,
                    _ => None,
                };
                // `Value := _File`: the file, or the nil interface.
                *value = match file {
                    Some(file) => state.borrow().element_value(&file),
                    None => Value::Empty,
                };
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        let vcl = vcl.clone();
        add(
            adapter,
            "AddNewFileName",
            -1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let count = args.values.len();
                if !(1..=3).contains(&count) {
                    return Ok(());
                }
                let name = String::from_utf8_lossy(&xedit_script::values::to_str(&args.values[0])?).into_owned();
                let flag = |index: usize| -> Result<bool, ValueError> {
                    args.values.get(index).map_or(Ok(false), xedit_script::values::to_bool)
                };
                let (light, medium) = (flag(1)?, flag(2)?);
                *value = match add_new_file_name(&state, &vcl, &name, light, medium)? {
                    Some(file) => state.borrow().element_value(&file),
                    None => Value::Empty,
                };
                Ok(())
            }),
        );
    }
    {
        let vcl = vcl.clone();
        add(
            adapter,
            "AddRequiredElementMasters",
            -1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                *value = Value::Bool(false);
                let Some(element) = interface_at(args, 0)? else {
                    return Ok(());
                };
                let Some(target) = interface_at(args, 1)?
                    .filter(|element| element.get_element_type() == ElementType::etFile)
                    .and_then(|file| file_impl(&file))
                else {
                    return Ok(());
                };
                let as_new = args.values.get(2).map_or(Ok(false), xedit_script::values::to_bool)?;
                let silent = if args.values.len() >= 4 {
                    xedit_script::values::to_bool(&args.values[3])?
                } else {
                    false
                };
                *value = Value::Bool(add_required_masters(&vcl, &element, &target, as_new, silent)?);
                Ok(())
            }),
        );
    }

    //=== The conflict status ================================================

    for (name, all) in [("ConflictAllForMainRecord", true), ("ConflictThisForMainRecord", false)] {
        let state = state.clone();
        add(
            adapter,
            name,
            1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let Some(record) = main_record_arg(args, 0)?.and_then(|record| record_impl(&(record as ElementRef)))
                else {
                    return Err(invalid_argument());
                };
                let status = conflict_level_for_main_record(&state, &record);
                *value = Value::Integer(if all {
                    status.all.ord() as i32
                } else {
                    status.this.ord() as i32
                });
                Ok(())
            }),
        );
    }
    for (name, all) in [("ConflictAllForNode", true), ("ConflictThisForNode", false)] {
        let state = state.clone();
        add(
            adapter,
            name,
            1,
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let Some(element) = interface_at(args, 0)? else {
                    return Err(invalid_argument());
                };
                let state = state.borrow();
                // A child group shows as its record's node.
                let element = crate::script::node_element(&element);
                if let Some(node) = node_of(&state, &element) {
                    let (conflict_all, conflict_this) = state
                        .host
                        .node_conflicts
                        .get(&node)
                        .copied()
                        .unwrap_or((ConflictAll::caUnknown, ConflictThis::ctUnknown));
                    *value = Value::Integer(if all {
                        conflict_all.ord() as i32
                    } else {
                        conflict_this.ord() as i32
                    });
                }
                Ok(())
            }),
        );
    }
    add(
        adapter,
        "ConflictAllForElements",
        -1,
        Rc::new(|value: &mut Value, args: &mut Args| {
            let mut elements: Vec<ElementRef> = Vec::new();
            let flags_at = match args.values.len() {
                3 => {
                    if let Some(list) = list_at(args, 0) {
                        let borrowed = list.borrow();
                        if let Some(items) = borrowed.list() {
                            for item in items.items() {
                                if let Some(element) = ScriptRunState::element_of(item) {
                                    elements.push(element);
                                }
                            }
                        }
                    }
                    1
                }
                4 => {
                    for index in 0..2 {
                        if let Some(element) = interface_at(args, index)? {
                            elements.push(element);
                        }
                    }
                    2
                }
                _ => 1,
            };
            *value = Value::Integer(ConflictAll::caUnknown.ord() as i32);
            if elements.is_empty() {
                return Ok(());
            }
            let sibling = args
                .values
                .get(flags_at)
                .map_or(Ok(false), xedit_script::values::to_bool)?;
            let injected = args
                .values
                .get(flags_at + 1)
                .map_or(Ok(false), xedit_script::values::to_bool)?;
            let files = run_files();
            let options = ConflictOptions::default();
            let mut context = ConflictContext::new(&options, &files);
            let result = context.conflict_level_for_elements(&elements, sibling, injected);
            *value = Value::Integer(result.ord() as i32);
            Ok(())
        }),
    );

    //=== The navigation tree ================================================

    {
        let state = state.clone();
        add(
            adapter,
            "JumpTo",
            2,
            Rc::new(move |_value: &mut Value, args: &mut Args| {
                let Some(record) = main_record_arg(args, 0)? else {
                    return Err(invalid_argument());
                };
                // `Boolean(Args.Values[1])`: the history direction, which a
                // headless run has no use for, still converts.
                xedit_script::values::to_bool(&args.values[1])?;
                let element: ElementRef = record;
                // `TMainRecordHistoryEntry.Show`: the record's node is
                // focused and selected alone, and shown, which paints it
                // (`vstNavBeforeItemErase` classifies a main record it
                // paints while its node is unclassified).
                // The rows the jump makes the GUI paint: the focused record
                // and the records whose nodes the expansion opens above it
                // (the `CELL` of a reference, the `WRLD` of an exterior
                // cell), the oracle's `host-paint` probe; the other rows its
                // window shows depend on the window's size and are not
                // modelled.
                let mut painted = vec![element.clone()];
                let mut current = element.get_container();
                while let Some(container) = current {
                    if container.get_element_type() == ElementType::etGroupRecord {
                        let shown = crate::script::node_element(&container);
                        if shown.get_element_type() == ElementType::etMainRecord {
                            painted.push(shown);
                        }
                    }
                    current = container.get_container();
                }
                for row in painted {
                    let unclassified = {
                        let state = state.borrow();
                        !state
                            .host
                            .node_conflicts
                            .get(&row.get_element_id())
                            .is_some_and(|(all, _)| *all != ConflictAll::caUnknown)
                    };
                    if unclassified && let Some(record) = record_impl(&row) {
                        let status = conflict_level_for_main_record(&state, &record);
                        state
                            .borrow_mut()
                            .host
                            .node_conflicts
                            .insert(row.get_element_id(), (status.all, status.this));
                    }
                }
                state.borrow_mut().host.selection = Some(vec![element]);
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        add(
            adapter,
            "wbSelectedFiles",
            1,
            Rc::new(move |_value: &mut Value, args: &mut Args| {
                let Some(list) = list_at(args, 0) else {
                    return Ok(());
                };
                let state = state.borrow();
                for node in selected_nodes(&state) {
                    let file = match node.get_element_type() {
                        ElementType::etFile => Some(node.clone()),
                        ElementType::etMainRecord => node
                            .get_file()
                            .and_then(|file| file.as_element_impl().and_then(ElementImpl::self_element_ref)),
                        _ => None,
                    };
                    let Some(file) = file else {
                        continue;
                    };
                    let mut borrowed = list.borrow_mut();
                    let Some(items) = borrowed.list_mut() else {
                        return Ok(());
                    };
                    let found = items.items().iter().any(|item| {
                        ScriptRunState::element_of(item)
                            .is_some_and(|known| known.get_element_id() == file.get_element_id())
                    });
                    if !found {
                        items.add(state.element_value(&file));
                    }
                }
                Ok(())
            }),
        );
    }
    {
        let state = state.clone();
        add(
            adapter,
            "wbSelectedFilesToFileNames",
            1,
            Rc::new(move |_value: &mut Value, args: &mut Args| {
                // `not (V2O(...) is TStrings) or not (... is TStringList)`.
                let list = strings_at(args, 0).filter(|object| {
                    let class = object.borrow().class();
                    let mut current = Some(class);
                    while let Some(class) = current {
                        if class.name.eq_ignore_ascii_case("TStringList") {
                            return true;
                        }
                        current = class.parent.clone();
                    }
                    false
                });
                let Some(list) = list else {
                    return Err(ValueError::interp_n(
                        IE_DIRECT_INVALID_ARGUMENT,
                        "Expected a TStrings or TStringsList",
                    ));
                };
                let state = state.borrow();
                for node in selected_nodes(&state) {
                    let file = match node.get_element_type() {
                        ElementType::etFile | ElementType::etMainRecord | ElementType::etGroupRecord => node.get_file(),
                        _ => None,
                    };
                    let Some(file) = file else {
                        continue;
                    };
                    let name = file_name_of(&file);
                    let mut borrowed = list.borrow_mut();
                    let Some(strings) = borrowed.string_list_mut() else {
                        return Ok(());
                    };
                    if strings.index_of(name.as_bytes()) == -1 {
                        let element = if node.get_element_type() == ElementType::etFile {
                            node.clone()
                        } else {
                            match file.as_element_impl().and_then(ElementImpl::self_element_ref) {
                                Some(element) => element,
                                None => continue,
                            }
                        };
                        strings.add_object(name.into_bytes(), state.element_value(&element));
                    }
                }
                Ok(())
            }),
        );
    }

    //=== LOD ================================================================

    for (name, kind) in [
        ("GenerateLODTES4", 4),
        ("GenerateLODTES5Trees", 1),
        ("GenerateLODTES5Objects", 2),
    ] {
        let state = state.clone();
        add(
            adapter,
            name,
            1,
            Rc::new(move |_value: &mut Value, args: &mut Args| {
                let Some(record) = main_record_arg(args, 0)? else {
                    return Err(invalid_argument());
                };
                let env = lod_env(&state);
                let result = if kind == 4 {
                    if globals::game_mode() != globals::GameMode::gmTES4 {
                        return Ok(());
                    }
                    let elapsed = || crate::script::elapsed_text(crate::script::progress_start().elapsed());
                    xedit_assets::lod::generate_lod_tes4(&env, &record, &elapsed)
                } else {
                    if !globals::is_skyrim() {
                        return Ok(());
                    }
                    let files = run_files();
                    xedit_assets::lod::generate_lod_tes5(&env, &record, kind == 1, kind == 2, &files)
                };
                result.map_err(|error| exception(error.0))
            }),
        );
    }
    add(
        adapter,
        "wbGetUVRangeTexturesList",
        3,
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let Some(meshes) = lod_list(&args.values[0]) else {
                return Err(invalid_argument());
            };
            let Some(textures_object) = strings_at(args, 1) else {
                return Err(invalid_argument());
            };
            let Some(mut textures) = lod_list(&args.values[1]) else {
                return Err(invalid_argument());
            };
            // `Single(Args.Values[2])`.
            let uv_range = xedit_script::values::to_f64(&args.values[2])? as f32;
            let mut known: HashMap<String, usize> = HashMap::new();
            for (text, _) in &textures.items {
                *known.entry(text.clone()).or_default() += 1;
            }
            let outcome = xedit_assets::lod::get_uv_range_textures_list(&meshes, &mut textures, uv_range);
            // The list the routine filled is the script's own: what it added
            // goes in (also when it stopped on an error), with the billboard
            // flag as the object.
            {
                let mut borrowed = textures_object.borrow_mut();
                if let Some(strings) = borrowed.string_list_mut() {
                    for (text, flag) in &textures.items {
                        if let Some(count) = known.get_mut(text)
                            && *count > 0
                        {
                            *count -= 1;
                            continue;
                        }
                        let object = if *flag == 0 {
                            Value::Empty
                        } else {
                            Value::Integer(*flag as i32)
                        };
                        strings.add_object(xedit_io::encoding::ansi_bytes(text), object);
                    }
                }
            }
            outcome.map_err(|error| exception(error.0))
        }),
    );
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::rc::Rc;

    use xedit_script::Interpreter;

    use crate::script::{C_UNIT, ScriptRunState};

    /// Every `xejviScriptAdapterHost.pas` function the ledger lists as
    /// covered is a registration of a script run's adapter: a ledger entry
    /// covered by a CLI command while the script function itself was missing
    /// (`AddRequiredElementMasters`, the two `...ForMainRecord` and
    /// `ConflictAllForElements` before issue #25) fails here.
    #[test]
    fn every_covered_host_function_of_the_ledger_is_registered() {
        let ledger = include_str!("../../../coverage/ledger.toml");
        let mut expected: BTreeSet<String> = BTreeSet::new();
        let mut current: Option<String> = None;
        for line in ledger.lines() {
            if let Some(rest) = line.strip_prefix("[script.\"xejviScriptAdapterHost:cUnit.") {
                current = rest.strip_suffix(":function\"]").map(str::to_owned);
                continue;
            }
            if line.starts_with('[') {
                current = None;
                continue;
            }
            if line == "status = \"covered\""
                && let Some(name) = current.take()
            {
                expected.insert(name.to_lowercase());
            }
        }
        assert_eq!(expected.len(), 56, "the ledger's host entries were not read");

        let mut interpreter = Interpreter::new();
        let class = interpreter.adapter.add_class(C_UNIT, "IwbElement", None);
        let state = Rc::new(RefCell::new(ScriptRunState::new(Vec::new(), class)));
        let model: Rc<RefCell<dyn crate::script_rtl::Vcl>> = Rc::new(RefCell::new(
            crate::script_vcl::interactive::VclModel::new("test", Vec::new()),
        ));
        crate::script::register_functions(&mut interpreter.adapter, &state, model);
        crate::script_units::register_units(&mut interpreter.adapter, &state);
        let registered: BTreeSet<String> = interpreter
            .adapter
            .registrations()
            .iter()
            .filter(|entry| entry.kind == "function")
            .filter_map(|entry| entry.identifier.as_ref().map(|name| name.to_lowercase()))
            .collect();
        let missing: Vec<&String> = expected.difference(&registered).collect();
        assert!(missing.is_empty(), "covered host functions not registered: {missing:?}");
    }
}
