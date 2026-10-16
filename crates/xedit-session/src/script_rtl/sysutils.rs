// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_SysUtils.pas (the
// `SysUtils` unit: the Exception class family, TSearchRec and the find
// functions, the file and path functions, the string functions, the Format
// family and the date functions)

//! `JvInterpreter_SysUtils.pas`: the `SysUtils` unit of the script dialect.
//!
//! The registrations are bound one for one. What the port answers
//! differently, all of it documented at the registration:
//!
//! * The four handlers whose upstream body is `NotImplemented`
//!   (`Exception.CreateFmt`, `CreateResFmt`, `CreateFmtHelp`,
//!   `CreateResFmtHelp`) raise the same text.
//! * `LoadPackage`/`UnloadPackage` have no package system to call; they
//!   raise `NotImplemented`.
//! * `GetLocaleStr`/`GetLocaleChar` answer the en-US locale values
//!   (`dateformat.rs`'s settings); `GetFormatSettings` has nothing to
//!   reload and does nothing.
//! * `ExceptObject`/`ExceptAddr` return the nil object/pointer: the
//!   interpreter's `except on E` handler binds the caught exception as a
//!   script variable, so an adapter call cannot see it.
//! * `Beep` is silent, `ShowException` writes nothing: a headless run has
//!   no console to beep or dialog to show.
//! * `DiskFree`/`DiskSize` return -1 (the invalid-drive answer): the port
//!   has no volume API.
//! * `FileRead`/`FileWrite` pass their buffer as a `var` *string*, the
//!   shape `TStream.ReadBuffer` documents (upstream reads and writes the
//!   variant's raw payload memory, which is garbage by construction), and
//!   the `Str*` pointer helpers return the text a `string(...)` cast
//!   upstream yields.
//! * `Exc` and `ExceptionErrorMessage` build the RTL's text shape from the
//!   exception object the port models.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use xedit_script::adapter::{Adapter, Args};
use xedit_script::values::{RecordInstance, ScriptObject, Value, ValueError, VarType, v2r};

use xedit_core::delphi;
use xedit_io::encoding::ansi_compare_text;

use super::datetime;
use super::format;
use super::{
    BYREF, EMPTY, arg, arg_bool, arg_char, arg_f64, arg_i32, arg_i64, arg_str, arg_text, constant, method, setter,
    text_value,
};

/// `cSysUtils = 'SysUtils'` (`JvInterpreter_SysUtils.pas:1742`).
const UNIT: &str = "SysUtils";

/// `SAbortError`.
const S_ABORT_ERROR: &str = "Operation aborted";
/// `SOutOfMemory`.
const S_OUT_OF_MEMORY: &str = "Out of memory";

/// Run-time cost of a `SysUtils` error path.
fn external(message: impl Into<String>) -> ValueError {
    ValueError::External(message.into())
}

/// A conversion the RTL reports as an `EConvertError` (`StrToFloat`,
/// `StrToInt`, the date parsers): the abort line names the class
/// (`Exception in unit ...: [EConvertError] ...`), as the oracle's does.
fn convert_error(message: impl Into<String>) -> ValueError {
    ValueError::external_class("EConvertError", message.into())
}

//=== Exception objects =======================================================

/// The object `Exception.Create`/`CreateHelp` produce: the registered class
/// (the `Exception` class and its descendants), the message and the help
/// context. The value's own `RefCell` carries the mutation, so the data
/// lives here directly.
pub struct ExceptionObject {
    class: Rc<xedit_script::values::ClassDef>,
    pub message: String,
    pub help_context: i32,
}

impl ExceptionObject {
    fn new(class: Rc<xedit_script::values::ClassDef>, message: String) -> ExceptionObject {
        ExceptionObject {
            class,
            message,
            help_context: 0,
        }
    }

    /// The object behind a value, when it is one.
    pub fn of(value: &Value) -> Option<Rc<RefCell<dyn ScriptObject>>> {
        let object = value.as_object()?.clone();
        object.borrow().as_any().downcast_ref::<ExceptionObject>()?;
        Some(object)
    }

    /// The message of the exception a value carries (`E.Message`).
    /// The message of an exception object: a raised one's, or the message
    /// a host or interpreter error carries into an `on E` handler (the
    /// interpreter's object, which reports it through `ScriptObject::message`).
    pub fn message_of(value: &Value) -> Option<String> {
        let object = value.as_object()?;
        let borrowed = object.borrow();
        borrowed
            .as_any()
            .downcast_ref::<ExceptionObject>()
            .map(|object| object.message.clone())
            .or_else(|| borrowed.message().map(str::to_owned))
    }
}

impl ScriptObject for ExceptionObject {
    fn class(&self) -> Rc<xedit_script::values::ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn message(&self) -> Option<&str> {
        Some(&self.message)
    }
}

/// A new exception value of `class`.
fn exception_value(class: &Rc<xedit_script::values::ClassDef>, message: String, help_context: i32) -> Value {
    let mut object = ExceptionObject::new(class.clone(), message);
    object.help_context = help_context;
    Value::Object(Rc::new(RefCell::new(object)))
}

//=== The find registry =======================================================

struct FindState {
    names: Vec<(String, std::fs::Metadata)>,
    next: usize,
    exclude_attr: i32,
}

thread_local! {
    /// The open `FindFirst` searches, keyed by the `FindHandle` a
    /// `TSearchRec` carries.
    static FINDS: RefCell<HashMap<i32, FindState>> = RefCell::new(HashMap::new());
    static NEXT_FIND: Cell<i32> = const { Cell::new(1) };
    /// The open file handles of `FileOpen`/`FileCreate`.
    static FILES: RefCell<HashMap<i32, std::fs::File>> = RefCell::new(HashMap::new());
    static NEXT_FILE: Cell<i32> = const { Cell::new(1) };
}

/// Windows attribute constants of the unit.
const FA_READONLY: i32 = 0x01;
const FA_HIDDEN: i32 = 0x02;
const FA_SYSFILE: i32 = 0x04;
const FA_DIRECTORY: i32 = 0x10;
const FA_ARCHIVE: i32 = 0x20;
const FA_ANY_FILE: i32 = 0x3F;

/// `ERROR_FILE_NOT_FOUND`.
const ERROR_FILE_NOT_FOUND: i32 = 2;
/// `ERROR_PATH_NOT_FOUND`.
const ERROR_PATH_NOT_FOUND: i32 = 3;
/// `ERROR_NO_MORE_FILES`.
const ERROR_NO_MORE_FILES: i32 = 18;

/// The attributes of a path, as the `TSearchRec.Attr` field carries them.
fn file_attributes(metadata: &std::fs::Metadata) -> i32 {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() as i32 & FA_ANY_FILE
    }
    #[cfg(not(windows))]
    {
        let mut attr = if metadata.is_dir() { FA_DIRECTORY } else { FA_ARCHIVE };
        if metadata.permissions().readonly() {
            attr |= FA_READONLY;
        }
        attr
    }
}

/// The DOS date of a metadata's modified time.
fn metadata_file_date(metadata: &std::fs::Metadata) -> i32 {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_secs_f64() / 86400.0)
        .map(|days| days + datetime::UNIX_EPOCH_TDATE)
        .unwrap_or(0.0);
    datetime::date_time_to_file_date(modified)
}

/// The `TSearchRec` field indexes (`AddRec` order).
const SR_TIME: usize = 0;
const SR_SIZE: usize = 1;
const SR_ATTR: usize = 2;
const SR_NAME: usize = 3;
const SR_EXCLUDE_ATTR: usize = 4;
const SR_FIND_HANDLE: usize = 5;

/// Writes one found entry into the `TSearchRec` a script passed by `var`.
fn fill_search_rec(
    instance: &mut RecordInstance,
    name: &str,
    metadata: &std::fs::Metadata,
    exclude_attr: i32,
    handle: i32,
) {
    instance.values[SR_TIME] = Value::Integer(metadata_file_date(metadata));
    instance.values[SR_SIZE] = Value::Integer(metadata.len().min(i32::MAX as u64) as i32);
    instance.values[SR_ATTR] = Value::Integer(file_attributes(metadata));
    instance.values[SR_NAME] = Value::Str(name.as_bytes().to_vec());
    instance.values[SR_EXCLUDE_ATTR] = Value::Integer(exclude_attr);
    instance.values[SR_FIND_HANDLE] = Value::Integer(handle);
}

/// The DOS wildcard match of `FindFirstFile`: the mask splits at its last
/// `.`, the name at its last `.`, and both parts match character by
/// character with `*` (any run) and `?` (one character), without case.
fn wildcard_match(mask: &str, name: &str) -> bool {
    if mask == "*" || mask == "*.*" {
        return true;
    }
    let (mask_stem, mask_ext) = split_wildcard(mask);
    let (name_stem, name_ext) = split_wildcard(name);
    match mask_ext {
        Some(mask_ext) => match_wildcard(mask_stem, name_stem) && match_wildcard(mask_ext, name_ext.unwrap_or("")),
        None => match_wildcard(mask_stem, name),
    }
}

fn split_wildcard(text: &str) -> (&str, Option<&str>) {
    match text.rfind('.') {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    }
}

fn match_wildcard(mask: &str, text: &str) -> bool {
    let mask: Vec<char> = mask.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let mut m = 0usize;
    let mut t = 0usize;
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if m < mask.len() && (mask[m] == '?' || mask[m].eq_ignore_ascii_case(&text[t])) {
            m += 1;
            t += 1;
        } else if m < mask.len() && mask[m] == '*' {
            star = Some((m, t));
            m += 1;
        } else if let Some((star_m, star_t)) = star {
            m = star_m + 1;
            t = star_t + 1;
            star = Some((star_m, star_t + 1));
        } else {
            return false;
        }
    }
    while m < mask.len() && mask[m] == '*' {
        m += 1;
    }
    m == mask.len()
}

/// `FindFirst`: the directory scan behind the `TSearchRec` API.
fn find_first(mask_path: &str, attr: i32) -> Result<(i32, FindState, String, std::fs::Metadata), i32> {
    let dir = delphi::extract_file_path(mask_path);
    let mask = delphi::path_file_name(mask_path);
    let dir = if dir.is_empty() { ".\\".to_owned() } else { dir };
    let mask = if mask.is_empty() {
        "*".to_owned()
    } else {
        mask.to_owned()
    };
    // The attributes not asked for are excluded (`ExcludeAttr` of the RTL).
    let exclude_attr = !attr & (FA_HIDDEN | FA_SYSFILE | FA_DIRECTORY | FA_VOLUME_ID);
    let entries = std::fs::read_dir(&dir).map_err(|_| ERROR_PATH_NOT_FOUND)?;
    let mut names: Vec<(String, std::fs::Metadata)> = Vec::new();
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        let attributes = file_attributes(&metadata);
        if attributes & exclude_attr != 0 {
            continue;
        }
        if !wildcard_match(&mask, &name) {
            continue;
        }
        names.push((name, metadata));
    }
    // The OS enumerates a directory in its index order, which for NTFS is
    // the name order with the Win32 case-insensitive collation; the port
    // sorts to the same order.
    names.sort_by(|a, b| ansi_compare_text(&a.0, &b.0));
    let Some((first_name, first_metadata)) = names.first().cloned() else {
        return Err(ERROR_FILE_NOT_FOUND);
    };
    let handle = NEXT_FIND.with(|cell| {
        let handle = cell.get();
        cell.set(handle + 1);
        handle
    });
    let state = FindState {
        names,
        next: 1,
        exclude_attr,
    };
    Ok((handle, state, first_name, first_metadata))
}

/// `FA_VOLUME_ID` (the deprecated `faVolumeID`, compiled out of the
/// registration list but part of the exclusion mask the RTL uses).
const FA_VOLUME_ID: i32 = 0x08;

/// `FM_*` open modes.
const FM_OPEN_READ: i32 = 0x0000;
const FM_OPEN_WRITE: i32 = 0x0001;
const FM_OPEN_READWRITE: i32 = 0x0002;
const FM_SHARE_EXCLUSIVE: i32 = 0x0010;
const FM_SHARE_DENY_WRITE: i32 = 0x0020;
const FM_SHARE_DENY_READ: i32 = 0x0030;
const FM_SHARE_DENY_NONE: i32 = 0x0040;

/// Opens with the Win32 share mode the `fmShare*` bits ask for.
fn open_shared(path: &Path, read: bool, write: bool, create: bool, mode: i32) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(read).write(write).create(create).truncate(create && write);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let share = match mode & 0xF0 {
            FM_SHARE_EXCLUSIVE => 0,
            FM_SHARE_DENY_WRITE => 0x1,
            FM_SHARE_DENY_READ => 0x2,
            FM_SHARE_DENY_NONE => 0x7,
            _ => 0,
        };
        options.share_mode(share);
    }
    #[cfg(not(windows))]
    let _ = mode;
    options.open(path)
}

/// Registers the unit.
pub fn register(adapter: &mut Adapter) {
    // `Exception` descends from `TObject` (registered by `System` first), so
    // `E.ClassName`, `ClassType` and `ClassNameIs` reach it as Delphi's RTTI
    // does for every object (`E.ClassName + ': ' + E.Message`, probe
    // `x-raise-03`).
    let t_object = adapter.find_class(b"TObject");
    let exception = adapter.add_class(UNIT, "Exception", t_object);
    // The constructor family, in the registration order of the unit.
    {
        let class = exception.clone();
        method(adapter, &exception, "Create", &[EMPTY], move |value, args| {
            let message = arg_text(args, 0)?;
            *value = exception_value(&class, message, 0);
            Ok(())
        });
    }
    method(adapter, &exception, "CreateFmt", &[EMPTY, EMPTY], |_value, _args| {
        // `NotImplemented('Exception.CreateFmt')` upstream (`:99`).
        Err(ValueError::not_implemented("Exception.CreateFmt"))
    });
    method(adapter, &exception, "CreateRes", &[EMPTY], |_value, args| {
        // The RTL loads the string with the identifier from the module's
        // resources; the port has no resource table, so the `EResNotFound`
        // answer is the one it can give.
        let ident = arg_i32(args, 0)?;
        Err(external(format!("Resource {ident} not found")))
    });
    method(adapter, &exception, "CreateResFmt", &[EMPTY, EMPTY], |_value, _args| {
        Err(ValueError::not_implemented("Exception.CreateResFmt"))
    });
    {
        let class = exception.clone();
        method(
            adapter,
            &exception,
            "CreateHelp",
            &[EMPTY, EMPTY],
            move |value, args| {
                let message = arg_text(args, 0)?;
                let help = arg_i32(args, 1)?;
                *value = exception_value(&class, message, help);
                Ok(())
            },
        );
    }
    method(
        adapter,
        &exception,
        "CreateFmtHelp",
        &[EMPTY, EMPTY, EMPTY],
        |_value, _args| Err(ValueError::not_implemented("Exception.CreateFmtHelp")),
    );
    method(adapter, &exception, "CreateResHelp", &[EMPTY, EMPTY], |_value, args| {
        let ident = arg_i32(args, 0)?;
        Err(external(format!("Resource {ident} not found")))
    });
    method(
        adapter,
        &exception,
        "CreateResFmtHelp",
        &[EMPTY, EMPTY, EMPTY],
        |_value, _args| Err(ValueError::not_implemented("Exception.CreateResFmtHelp")),
    );
    method(adapter, &exception, "HelpContext", &[], |value, args| {
        *value = Value::Integer(
            args.obj
                .as_ref()
                .and_then(Value::as_object)
                .map(|object| {
                    let borrowed = object.borrow();
                    borrowed
                        .as_any()
                        .downcast_ref::<ExceptionObject>()
                        .map(|object| object.help_context)
                        .unwrap_or(0)
                })
                .unwrap_or(0),
        );
        Ok(())
    });
    setter(adapter, &exception, "HelpContext", &[EMPTY], |value, _args| {
        let help_context = xedit_script::values::to_i32(value)?;
        if let Some(object) = _args.obj.as_ref().and_then(Value::as_object) {
            let mut borrowed = object.borrow_mut();
            if let Some(object) = borrowed.as_any_mut().downcast_mut::<ExceptionObject>() {
                object.help_context = help_context;
            }
        }
        Ok(())
    });
    method(adapter, &exception, "Message", &[], |value, args| {
        // A raised `Exception` object's message, or the error an
        // interpreter/external exception carries (`E.Message` inside `on
        // E: Exception`, probes `x-raise-04` to `x-raise-10`).
        let message = args.obj.as_ref().and_then(ExceptionObject::message_of).or_else(|| {
            args.obj
                .as_ref()
                .and_then(Value::as_object)
                .and_then(|object| object.borrow().message().map(str::to_owned))
        });
        *value = text_value(message.unwrap_or_default().into_bytes());
        Ok(())
    });
    setter(adapter, &exception, "Message", &[EMPTY], |value, args| {
        let message = String::from_utf8_lossy(&xedit_script::values::to_str(value)?).into_owned();
        if let Some(object) = args.obj.as_ref().and_then(Value::as_object) {
            let mut borrowed = object.borrow_mut();
            if let Some(object) = borrowed.as_any_mut().downcast_mut::<ExceptionObject>() {
                object.message = message;
            }
        }
        Ok(())
    });

    // The exception classes, in the registration order of the unit and with
    // the Delphi hierarchy (`EAbort` under `Exception`, `EDivByZero` under
    // `EIntError`, and so on), which is what an `on E: T` clause matches.
    let class = |adapter: &mut Adapter, name: &str, parent: &Rc<xedit_script::values::ClassDef>| {
        adapter.add_class(UNIT, name, Some(parent.clone()))
    };
    let e_abort = class(adapter, "EAbort", &exception);
    let e_out_of_memory = class(adapter, "EOutOfMemory", &exception);
    let e_in_out_error = class(adapter, "EInOutError", &exception);
    let e_int_error = class(adapter, "EIntError", &exception);
    class(adapter, "EDivByZero", &e_int_error);
    class(adapter, "ERangeError", &e_int_error);
    class(adapter, "EIntOverflow", &e_int_error);
    let e_math_error = class(adapter, "EMathError", &exception);
    class(adapter, "EInvalidOp", &e_math_error);
    class(adapter, "EZeroDivide", &e_math_error);
    class(adapter, "EOverflow", &e_math_error);
    class(adapter, "EUnderflow", &e_math_error);
    class(adapter, "EInvalidPointer", &exception);
    class(adapter, "EInvalidCast", &exception);
    class(adapter, "EConvertError", &exception);
    let e_external = class(adapter, "EExternalException", &exception);
    class(adapter, "EAccessViolation", &e_external);
    class(adapter, "EPrivilege", &e_external);
    class(adapter, "EControlC", &e_external);
    let e_variant = class(adapter, "EVariantError", &exception);
    class(adapter, "EPropReadOnly", &e_variant);
    class(adapter, "EPropWriteOnly", &e_variant);
    class(adapter, "EAssertionFailed", &exception);
    class(adapter, "EAbstractError", &exception);
    class(adapter, "EIntfCastError", &exception);
    let e_invalid_container = class(adapter, "EInvalidContainer", &exception);
    class(adapter, "EInvalidInsert", &e_invalid_container);
    class(adapter, "EPackageError", &exception);
    class(adapter, "EOSError", &exception);
    let _ = (e_abort, e_out_of_memory, e_in_out_error);

    register_functions(adapter);

    // The file open mode constants.
    constant(adapter, UNIT, "fmOpenRead", FM_OPEN_READ);
    constant(adapter, UNIT, "fmOpenWrite", FM_OPEN_WRITE);
    constant(adapter, UNIT, "fmOpenReadWrite", FM_OPEN_READWRITE);
    constant(adapter, UNIT, "fmShareCompat", 0);
    constant(adapter, UNIT, "fmShareExclusive", FM_SHARE_EXCLUSIVE);
    constant(adapter, UNIT, "fmShareDenyWrite", FM_SHARE_DENY_WRITE);
    constant(adapter, UNIT, "fmShareDenyRead", FM_SHARE_DENY_READ);
    constant(adapter, UNIT, "fmShareDenyNone", FM_SHARE_DENY_NONE);
    // The file attribute constants.
    constant(adapter, UNIT, "faReadOnly", FA_READONLY);
    constant(adapter, UNIT, "faHidden", FA_HIDDEN);
    constant(adapter, UNIT, "faSysFile", FA_SYSFILE);
    constant(adapter, UNIT, "faDirectory", FA_DIRECTORY);
    constant(adapter, UNIT, "faArchive", FA_ARCHIVE);
    constant(adapter, UNIT, "faAnyFile", FA_ANY_FILE);

    // `AddRec(cSysUtils, 'TSearchRec', SizeOf(TSearchRec), [RFD(...)] ...)`:
    // the port keeps the values per field, so the offsets are the layout
    // metadata of the 64-bit build the RFD calls compute upstream.
    adapter.add_rec(
        UNIT,
        "TSearchRec",
        40,
        vec![
            field("Time", 0, VarType::Integer),
            field("Size", 4, VarType::Integer),
            field("Attr", 16, VarType::Integer),
            field("Name", 20, VarType::Str),
            field("ExcludeAttr", 24, VarType::Integer),
            field("FindHandle", 28, VarType::Integer),
        ],
    );
}

fn field(name: &str, offset: i64, typ: VarType) -> xedit_script::values::RecordField {
    xedit_script::values::RecordField {
        name: name.to_owned(),
        offset,
        typ,
        data_type: None,
    }
}

fn register_functions(adapter: &mut Adapter) {
    // A `SysUtils` function.
    fn f(
        adapter: &mut Adapter,
        identifier: &str,
        params: &[xedit_script::values::ParamType],
        func: impl Fn(&mut Value, &mut Args) -> Result<(), ValueError> + 'static,
    ) {
        super::function(adapter, UNIT, identifier, params, func);
    }

    //=== Strings ============================================================

    f(adapter, "UpperCase", &[EMPTY], |value, args| {
        *value = text_value(upper_case(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "LowerCase", &[EMPTY], |value, args| {
        *value = text_value(lower_case(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "CompareStr", &[EMPTY, EMPTY], |value, args| {
        *value = Value::Integer(compare(&arg_str(args, 0)?, &arg_str(args, 1)?, false, None));
        Ok(())
    });
    f(adapter, "CompareMem", &[EMPTY, EMPTY, EMPTY], |value, args| {
        // `CompareMem(P1, P2, Length)`: the variant payload pointers; the
        // port compares the two values' byte forms over `Length` bytes.
        let a = values_payload(arg(args, 0));
        let b = values_payload(arg(args, 1));
        let count = arg_i32(args, 2)?.max(0) as usize;
        let equal = (0..count).all(|index| a.get(index).copied().unwrap_or(0) == b.get(index).copied().unwrap_or(0));
        *value = Value::Bool(equal);
        Ok(())
    });
    f(adapter, "CompareText", &[EMPTY, EMPTY], |value, args| {
        *value = Value::Integer(compare(&arg_str(args, 0)?, &arg_str(args, 1)?, true, None));
        Ok(())
    });
    f(adapter, "ExtractQuotedString", &[EMPTY, EMPTY], |value, args| {
        let text = arg_str(args, 0)?;
        let quote = arg_char(args, 1);
        *value = text_value(extract_quoted(&text, quote).1);
        Ok(())
    });
    f(adapter, "AnsiUpperCase", &[EMPTY], |value, args| {
        *value = text_value(upper_case(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "AnsiLowerCase", &[EMPTY], |value, args| {
        *value = text_value(lower_case(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "AnsiCompareStr", &[EMPTY, EMPTY], |value, args| {
        let a = String::from_utf8_lossy(&arg_str(args, 0)?).into_owned();
        let b = String::from_utf8_lossy(&arg_str(args, 1)?).into_owned();
        *value = Value::Integer(match ansi_compare_text(&a, &b) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        });
        Ok(())
    });
    f(adapter, "AnsiCompareText", &[EMPTY, EMPTY], |value, args| {
        let a = String::from_utf8_lossy(&arg_str(args, 0)?).into_owned();
        let b = String::from_utf8_lossy(&arg_str(args, 1)?).into_owned();
        *value = Value::Integer(match ansi_compare_text(&a, &b) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        });
        Ok(())
    });
    f(adapter, "AnsiStrComp", &[EMPTY, EMPTY], |value, args| {
        *value = Value::Integer(compare(&arg_str(args, 0)?, &arg_str(args, 1)?, false, None));
        Ok(())
    });
    f(adapter, "AnsiStrIComp", &[EMPTY, EMPTY], |value, args| {
        *value = Value::Integer(compare(&arg_str(args, 0)?, &arg_str(args, 1)?, true, None));
        Ok(())
    });
    f(adapter, "AnsiStrLComp", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let max = arg_i32(args, 2)?.max(0) as usize;
        *value = Value::Integer(compare(&arg_str(args, 0)?, &arg_str(args, 1)?, false, Some(max)));
        Ok(())
    });
    f(adapter, "AnsiStrLIComp", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let max = arg_i32(args, 2)?.max(0) as usize;
        *value = Value::Integer(compare(&arg_str(args, 0)?, &arg_str(args, 1)?, true, Some(max)));
        Ok(())
    });
    f(adapter, "AnsiStrLower", &[EMPTY], |value, args| {
        *value = text_value(lower_case(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "AnsiStrUpper", &[EMPTY], |value, args| {
        *value = text_value(upper_case(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "AnsiLastChar", &[EMPTY], |value, args| {
        *value = text_value(arg_str(args, 0)?.last().copied().into_iter().collect::<Vec<u8>>());
        Ok(())
    });
    f(adapter, "AnsiStrLastChar", &[EMPTY], |value, args| {
        *value = text_value(arg_str(args, 0)?.last().copied().into_iter().collect::<Vec<u8>>());
        Ok(())
    });
    f(adapter, "Trim", &[EMPTY], |value, args| {
        *value = text_value(trim(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "TrimLeft", &[EMPTY], |value, args| {
        *value = text_value(trim_left(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "TrimRight", &[EMPTY], |value, args| {
        *value = text_value(trim_right(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "QuotedStr", &[EMPTY], |value, args| {
        *value = text_value(quoted_str(&arg_str(args, 0)?, b'\''));
        Ok(())
    });
    f(adapter, "AnsiQuotedStr", &[EMPTY, EMPTY], |value, args| {
        *value = text_value(quoted_str(&arg_str(args, 0)?, arg_char(args, 1)));
        Ok(())
    });
    f(adapter, "AnsiExtractQuotedStr", &[BYREF, EMPTY], |value, args| {
        let text = arg_str(args, 0)?;
        let quote = arg_char(args, 1);
        let (rest, extracted) = extract_quoted(&text, quote);
        args.values[0] = Value::Str(rest);
        *value = text_value(extracted);
        Ok(())
    });
    f(adapter, "AdjustLineBreaks", &[EMPTY], |value, args| {
        *value = text_value(adjust_line_breaks(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "IsValidIdent", &[EMPTY], |value, args| {
        *value = Value::Bool(is_valid_ident(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "IntToStr", &[EMPTY], |value, args| {
        // `IntToStr(Args.Values[0])` takes the `Int64` overload
        // (`IntToStr(4294967296)` prints it, probe `x-259`); an object
        // or a double beyond the range is read without an error upstream
        // (its payload, `x-250`, `x-262`), which the port stands in with
        // the pointer-free reading of the value.
        let number = match arg(args, 0) {
            // An interface does not convert (`Type mismatch`, probe
            // `x-251`); another object reads its payload (probe `x-250`).
            Value::Object(object) if object.borrow().is_interface() => return Err(ValueError::Variant),
            Value::Object(_) | Value::Class(_) | Value::Pointer(_) => 0,
            Value::Double(v) | Value::Date(v) if !v.is_finite() || v.abs() >= 9.223_372_036_854_776e18 => i64::MIN,
            other => xedit_script::values::to_i64(other)?,
        };
        *value = text_value(number.to_string().into_bytes());
        Ok(())
    });
    f(adapter, "IntToHex", &[EMPTY, EMPTY], |value, args| {
        // The `UInt64` overload takes the variant: a negative value is the
        // conversion's overflow, an `EVariantError` (`IntToHex(-1, 8)` is
        // `Type mismatch` in an expression, probe `x-073`).
        let number = arg_i64(args, 0)?;
        if number < 0 {
            return Err(xedit_script::values::overflow_error(arg(args, 0), "UInt64"));
        }
        *value = text_value(format::int_to_hex(number, arg_i32(args, 1)?).into_bytes());
        Ok(())
    });
    f(adapter, "StrToInt", &[EMPTY], |value, args| {
        // An object reads as `''` here (probe `x-273`).
        let text = match arg(args, 0) {
            Value::Object(_) | Value::Class(_) => String::new(),
            _ => arg_text(args, 0)?,
        };
        // `EConvertError` (probe `x-007`).
        *value = Value::Integer(
            format::str_to_int(&text).map_err(|message| ValueError::external_class("EConvertError", message))?,
        );
        Ok(())
    });
    f(adapter, "StrToIntDef", &[EMPTY, EMPTY], |value, args| {
        let text = arg_text(args, 0)?;
        let default = arg_i32(args, 1)?;
        *value = Value::Integer(format::str_to_int(&text).unwrap_or(default));
        Ok(())
    });
    f(adapter, "LoadStr", &[EMPTY], |value, _args| {
        // No string table: the RTL returns `''` for an identifier it cannot
        // resolve, which is the answer the port gives for every one.
        *value = text_value(Vec::new());
        Ok(())
    });
    f(adapter, "AllocMem", &[EMPTY], |value, args| {
        // `AllocMem(Size)`: a zero-filled block; the port returns a string
        // of that many zero bytes (the value model has no raw memory).
        let size = arg_i32(args, 0)?.max(0) as usize;
        *value = Value::Str(vec![0; size]);
        Ok(())
    });

    //=== The `Str*` pointer helpers =========================================

    f(adapter, "StrLen", &[EMPTY], |value, args| {
        *value = Value::Integer(arg_str(args, 0)?.len() as i32);
        Ok(())
    });
    f(adapter, "StrEnd", &[EMPTY], |value, _args| {
        *value = text_value(Vec::new());
        Ok(())
    });
    f(adapter, "StrMove", &[EMPTY, EMPTY, EMPTY], |value, args| {
        // The destination is a buffer; the text it holds after the move is
        // the source's first `Count` characters.
        let source = arg_str(args, 1)?;
        let count = arg_i32(args, 2)?.max(0) as usize;
        *value = text_value(source[..count.min(source.len())].to_vec());
        Ok(())
    });
    f(adapter, "StrCopy", &[EMPTY, EMPTY], |value, args| {
        *value = text_value(arg_str(args, 1)?);
        Ok(())
    });
    f(adapter, "StrECopy", &[EMPTY, EMPTY], |value, args| {
        *value = text_value(arg_str(args, 1)?);
        Ok(())
    });
    f(adapter, "StrLCopy", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let source = arg_str(args, 1)?;
        let max = arg_i32(args, 2)?.max(0) as usize;
        *value = text_value(source[..max.min(source.len())].to_vec());
        Ok(())
    });
    f(adapter, "StrPCopy", &[EMPTY, EMPTY], |value, args| {
        *value = text_value(arg_str(args, 1)?);
        Ok(())
    });
    f(adapter, "StrPLCopy", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let source = arg_str(args, 1)?;
        let max = arg_i32(args, 2)?.max(0) as usize;
        *value = text_value(source[..max.min(source.len())].to_vec());
        Ok(())
    });
    f(adapter, "StrCat", &[EMPTY, EMPTY], |value, args| {
        let mut text = arg_str(args, 0)?;
        text.extend_from_slice(&arg_str(args, 1)?);
        *value = text_value(text);
        Ok(())
    });
    f(adapter, "StrLCat", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let mut text = arg_str(args, 0)?;
        text.extend_from_slice(&arg_str(args, 1)?);
        let max = arg_i32(args, 2)?.max(0) as usize;
        text.truncate(max);
        *value = text_value(text);
        Ok(())
    });
    f(adapter, "StrComp", &[EMPTY, EMPTY], |value, args| {
        *value = Value::Integer(compare(&arg_str(args, 0)?, &arg_str(args, 1)?, false, None));
        Ok(())
    });
    f(adapter, "StrIComp", &[EMPTY, EMPTY], |value, args| {
        *value = Value::Integer(compare(&arg_str(args, 0)?, &arg_str(args, 1)?, true, None));
        Ok(())
    });
    f(adapter, "StrLComp", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let max = arg_i32(args, 2)?.max(0) as usize;
        *value = Value::Integer(compare(&arg_str(args, 0)?, &arg_str(args, 1)?, false, Some(max)));
        Ok(())
    });
    f(adapter, "StrLIComp", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let max = arg_i32(args, 2)?.max(0) as usize;
        *value = Value::Integer(compare(&arg_str(args, 0)?, &arg_str(args, 1)?, true, Some(max)));
        Ok(())
    });
    f(adapter, "StrScan", &[EMPTY, EMPTY], |value, args| {
        let text = arg_str(args, 0)?;
        let ch = arg_char(args, 1);
        *value = text_value(scan(&text, ch, false));
        Ok(())
    });
    f(adapter, "StrRScan", &[EMPTY, EMPTY], |value, args| {
        let text = arg_str(args, 0)?;
        let ch = arg_char(args, 1);
        *value = text_value(scan(&text, ch, true));
        Ok(())
    });
    f(adapter, "StrPos", &[EMPTY, EMPTY], |value, args| {
        // `StrPos(Str1, Str2)`: the tail of `Str1` from `Str2`.
        let haystack = arg_str(args, 0)?;
        let needle = arg_str(args, 1)?;
        *value = text_value(pos(&needle, &haystack));
        Ok(())
    });
    f(adapter, "StrUpper", &[EMPTY], |value, args| {
        *value = text_value(upper_case(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "StrLower", &[EMPTY], |value, args| {
        *value = text_value(lower_case(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "StrPas", &[EMPTY], |value, args| {
        *value = text_value(arg_str(args, 0)?);
        Ok(())
    });
    f(adapter, "StrAlloc", &[EMPTY], |value, args| {
        let size = arg_i32(args, 0)?.max(0) as usize;
        *value = Value::Str(vec![0; size]);
        Ok(())
    });
    f(adapter, "StrBufSize", &[EMPTY], |value, args| {
        // The allocation's size; the port's strings carry their length plus
        // the terminator.
        *value = Value::Integer(arg_str(args, 0)?.len() as i32 + 1);
        Ok(())
    });
    f(adapter, "StrNew", &[EMPTY], |value, args| {
        *value = text_value(arg_str(args, 0)?);
        Ok(())
    });
    f(adapter, "StrDispose", &[EMPTY], |_value, _args| {
        // The port's strings free with their last reference.
        Ok(())
    });

    //=== Format =============================================================

    f(adapter, "Format", &[EMPTY, EMPTY], |value, args| {
        let format = arg_str(args, 0)?;
        let items = args.open_array(1)?;
        *value = Value::Str(format::format_text(&format, &items)?);
        Ok(())
    });
    f(adapter, "FmtStr", &[BYREF, EMPTY, EMPTY], |value, args| {
        let format = arg_str(args, 1)?;
        let items = args.open_array(2)?;
        let text = format::format_text(&format, &items)?;
        args.values[0] = Value::Str(text.clone());
        *value = Value::Str(text);
        Ok(())
    });
    f(adapter, "StrFmt", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let format = arg_str(args, 1)?;
        let items = args.open_array(2)?;
        *value = Value::Str(format::format_text(&format, &items)?);
        Ok(())
    });
    f(adapter, "StrLFmt", &[EMPTY, EMPTY, EMPTY, EMPTY], |value, args| {
        let format = arg_str(args, 2)?;
        let items = args.open_array(3)?;
        let mut text = format::format_text(&format, &items)?;
        let max = arg_i32(args, 1)?.max(0) as usize;
        text.truncate(max);
        *value = Value::Str(text);
        Ok(())
    });
    f(
        adapter,
        "FormatBuf",
        &[BYREF, EMPTY, EMPTY, EMPTY, EMPTY],
        |value, args| {
            let format = arg_str(args, 2)?;
            let items = args.open_array(4)?;
            let text = format::format_text(&format, &items)?;
            args.values[0] = Value::Str(text.clone());
            // The number of characters written including the terminator.
            *value = Value::Integer(text.len() as i32 + 1);
            Ok(())
        },
    );
    f(adapter, "FloatToStr", &[EMPTY], |value, args| {
        // An object reads its payload upstream, without an error (probe
        // `x-267`).
        let number = match arg(args, 0) {
            Value::Object(object) if !object.borrow().is_interface() => 0.0,
            Value::Class(_) | Value::Pointer(_) => 0.0,
            other => xedit_script::values::to_f64(other)?,
        };
        *value = text_value(delphi::float_to_str(number).into_bytes());
        Ok(())
    });
    f(adapter, "CurrToStr", &[EMPTY], |value, args| {
        *value = text_value(format::curr_to_str(arg_f64(args, 0)?).into_bytes());
        Ok(())
    });
    f(adapter, "FloatToStrF", &[EMPTY, EMPTY, EMPTY, EMPTY], |value, args| {
        *value = text_value(
            format::float_to_str_f(
                arg_f64(args, 0)?,
                format::FloatFormat::from_code(arg_i32(args, 1)?),
                arg_i32(args, 2)?,
                arg_i32(args, 3)?,
            )
            .into_bytes(),
        );
        Ok(())
    });
    f(adapter, "CurrToStrF", &[EMPTY, EMPTY, EMPTY], |value, args| {
        *value = text_value(
            format::float_to_str_f(
                arg_f64(args, 0)?,
                format::FloatFormat::from_code(arg_i32(args, 1)?),
                15,
                arg_i32(args, 2)?,
            )
            .into_bytes(),
        );
        Ok(())
    });
    f(adapter, "FormatFloat", &[EMPTY, EMPTY], |value, args| {
        let picture = String::from_utf8_lossy(&arg_str(args, 0)?).into_owned();
        *value = text_value(format::format_float(&picture, arg_f64(args, 1)?).into_bytes());
        Ok(())
    });
    f(adapter, "FormatCurr", &[EMPTY, EMPTY], |value, args| {
        let picture = String::from_utf8_lossy(&arg_str(args, 0)?).into_owned();
        *value = text_value(format::format_float(&picture, arg_f64(args, 1)?).into_bytes());
        Ok(())
    });
    f(adapter, "StrToFloat", &[EMPTY], |value, args| {
        let text = arg_text(args, 0)?;
        *value = Value::Double(
            format::str_to_float(&text).map_err(|message| ValueError::external_class("EConvertError", message))?,
        );
        Ok(())
    });
    f(adapter, "StrToCurr", &[EMPTY], |value, args| {
        let text = arg_text(args, 0)?;
        *value = Value::Double(format::str_to_float(&text).map_err(convert_error)?);
        Ok(())
    });

    //=== Files ==============================================================

    f(adapter, "FileOpen", &[EMPTY, EMPTY], |value, args| {
        let path = arg_text(args, 0)?;
        let mode = arg_i32(args, 1)?;
        let access = mode & 0x0F;
        let read = access == FM_OPEN_READ || access == FM_OPEN_READWRITE;
        let write = access == FM_OPEN_WRITE || access == FM_OPEN_READWRITE;
        *value = Value::Integer(match open_shared(Path::new(&path), read, write, false, mode) {
            Ok(file) => store_file(file),
            Err(_) => -1,
        });
        Ok(())
    });
    f(adapter, "FileCreate", &[EMPTY], |value, args| {
        let path = arg_text(args, 0)?;
        *value = Value::Integer(match open_shared(Path::new(&path), true, true, true, 0) {
            Ok(file) => store_file(file),
            Err(_) => -1,
        });
        Ok(())
    });
    f(adapter, "FileRead", &[EMPTY, BYREF, EMPTY], |value, args| {
        use std::io::Read;
        // The buffer arrives as a variant; the port reads into a string the
        // way `TStream.ReadBuffer` does (upstream reads the variant's raw
        // payload).
        let handle = arg_i32(args, 0)?;
        let count = arg_i32(args, 2)?.max(0) as usize;
        let read = FILES.with(|files| {
            let mut files = files.borrow_mut();
            let Some(file) = files.get_mut(&handle) else {
                return Err(());
            };
            let mut buffer = vec![0u8; count];
            let read = file.read(&mut buffer).map_err(|_| ())?;
            buffer.truncate(read);
            Ok(buffer)
        });
        match read {
            Ok(buffer) => {
                let read = buffer.len() as i32;
                args.values[1] = Value::Str(buffer);
                *value = Value::Integer(read);
            }
            Err(()) => *value = Value::Integer(-1),
        }
        Ok(())
    });
    f(adapter, "FileWrite", &[EMPTY, EMPTY, EMPTY], |value, args| {
        use std::io::Write;
        let handle = arg_i32(args, 0)?;
        let buffer = values_payload(arg(args, 1));
        let count = arg_i32(args, 2)?.max(0) as usize;
        let data = &buffer[..count.min(buffer.len())];
        let written = FILES.with(|files| {
            let mut files = files.borrow_mut();
            let Some(file) = files.get_mut(&handle) else {
                return Err(());
            };
            match file.write(data) {
                Ok(written) => Ok(written as i32),
                Err(_) => Err(()),
            }
        });
        *value = Value::Integer(written.unwrap_or(-1));
        Ok(())
    });
    f(adapter, "FileSeek", &[EMPTY, EMPTY, EMPTY], |value, args| {
        use std::io::{Seek, SeekFrom};
        let handle = arg_i32(args, 0)?;
        let offset = arg_i64(args, 1)?;
        let origin = arg_i32(args, 2)?;
        let seek = match origin {
            1 => SeekFrom::Current(offset),
            2 => SeekFrom::End(offset),
            _ => SeekFrom::Start(offset.max(0) as u64),
        };
        let position = FILES.with(|files| {
            let mut files = files.borrow_mut();
            let Some(file) = files.get_mut(&handle) else {
                return Err(());
            };
            file.seek(seek).map(|position| position as i64).map_err(|_| ())
        });
        *value = Value::Integer(match position {
            Ok(position) => position as i32,
            Err(()) => -1,
        });
        Ok(())
    });
    f(adapter, "FileClose", &[EMPTY], |_value, args| {
        let handle = arg_i32(args, 0)?;
        FILES.with(|files| files.borrow_mut().remove(&handle));
        Ok(())
    });
    f(adapter, "FileAge", &[EMPTY], |value, args| {
        let path = arg_text(args, 0)?;
        *value = Value::Integer(
            std::fs::metadata(&path)
                .map(|metadata| metadata_file_date(&metadata))
                .unwrap_or(-1),
        );
        Ok(())
    });
    f(adapter, "FileExists", &[EMPTY], |value, args| {
        let path = arg_text(args, 0)?;
        *value = Value::Bool(Path::new(&path).is_file());
        Ok(())
    });
    f(adapter, "FindFirst", &[EMPTY, EMPTY, BYREF], |value, args| {
        let pattern = arg_text(args, 0)?;
        let attr = arg_i32(args, 1)?;
        let record = v2r(arg(args, 2))?;
        match find_first(&pattern, attr) {
            Ok((handle, state, name, metadata)) => {
                let exclude = state.exclude_attr;
                FINDS.with(|finds| finds.borrow_mut().insert(handle, state));
                fill_search_rec(&mut record.borrow_mut(), &name, &metadata, exclude, handle);
                *value = Value::Integer(0);
            }
            Err(code) => *value = Value::Integer(code),
        }
        Ok(())
    });
    f(adapter, "FindNext", &[BYREF], |value, args| {
        let record = v2r(arg(args, 0))?;
        let handle = match &record.borrow().values[SR_FIND_HANDLE] {
            Value::Integer(handle) => *handle,
            _ => 0,
        };
        let next = FINDS.with(|finds| {
            let mut finds = finds.borrow_mut();
            let state = finds.get_mut(&handle)?;
            let entry = state.names.get(state.next).cloned();
            state.next += 1;
            entry
        });
        match next {
            Some((name, metadata)) => {
                let exclude =
                    FINDS.with(|finds| finds.borrow().get(&handle).map(|state| state.exclude_attr).unwrap_or(0));
                fill_search_rec(&mut record.borrow_mut(), &name, &metadata, exclude, handle);
                *value = Value::Integer(0);
            }
            None => {
                record.borrow_mut().values[SR_NAME] = Value::Str(Vec::new());
                *value = Value::Integer(ERROR_NO_MORE_FILES);
            }
        }
        Ok(())
    });
    f(adapter, "FindClose", &[BYREF], |_value, args| {
        let record = v2r(arg(args, 0))?;
        let handle = match &record.borrow().values[SR_FIND_HANDLE] {
            Value::Integer(handle) => *handle,
            _ => 0,
        };
        FINDS.with(|finds| finds.borrow_mut().remove(&handle));
        record.borrow_mut().values[SR_FIND_HANDLE] = Value::Integer(0);
        Ok(())
    });
    f(adapter, "FileGetDate", &[EMPTY], |value, args| {
        let handle = arg_i32(args, 0)?;
        let date = FILES.with(|files| {
            files
                .borrow()
                .get(&handle)
                .and_then(|file| file.metadata().ok())
                .map(|metadata| metadata_file_date(&metadata))
        });
        *value = Value::Integer(date.unwrap_or(-1));
        Ok(())
    });
    f(adapter, "FileSetDate", &[EMPTY, EMPTY], |value, args| {
        // `FileSetDate(VarToStr(Args.Values[0]), Args.Values[1])`: the first
        // argument is a file name.
        let path = arg_text(args, 0)?;
        let age = arg_i32(args, 1)?;
        let date = datetime::file_date_to_date_time(age);
        *value = Value::Integer(set_file_date(&path, date).unwrap_or(-1));
        Ok(())
    });
    f(adapter, "FileGetAttr", &[EMPTY], |value, args| {
        let path = arg_text(args, 0)?;
        *value = Value::Integer(
            std::fs::metadata(&path)
                .map(|metadata| file_attributes(&metadata))
                .unwrap_or(-1),
        );
        Ok(())
    });
    f(adapter, "FileSetAttr", &[EMPTY, EMPTY], |value, args| {
        let path = arg_text(args, 0)?;
        let attr = arg_i32(args, 1)?;
        // The read-only bit maps to the file permissions; the other bits
        // need the Win32 attribute API the port does not call.
        let result = (|| -> std::io::Result<()> {
            let metadata = std::fs::metadata(&path)?;
            let mut permissions = metadata.permissions();
            permissions.set_readonly(attr & FA_READONLY != 0);
            std::fs::set_permissions(&path, permissions)
        })();
        *value = Value::Integer(if result.is_ok() { 0 } else { -1 });
        Ok(())
    });
    f(adapter, "DeleteFile", &[EMPTY], |value, args| {
        let path = arg_text(args, 0)?;
        *value = Value::Bool(std::fs::remove_file(&path).is_ok());
        Ok(())
    });
    f(adapter, "RenameFile", &[EMPTY, EMPTY], |value, args| {
        let old = arg_text(args, 0)?;
        let new = arg_text(args, 1)?;
        *value = Value::Bool(std::fs::rename(&old, &new).is_ok());
        Ok(())
    });
    f(adapter, "ChangeFileExt", &[EMPTY, EMPTY], |value, args| {
        *value = text_value(delphi::change_file_ext(&arg_text(args, 0)?, &arg_text(args, 1)?).into_bytes());
        Ok(())
    });
    f(adapter, "ExtractFilePath", &[EMPTY], |value, args| {
        *value = text_value(delphi::extract_file_path(&arg_text(args, 0)?).into_bytes());
        Ok(())
    });
    f(adapter, "ExtractFileDir", &[EMPTY], |value, args| {
        *value = text_value(extract_file_dir(&arg_text(args, 0)?).into_bytes());
        Ok(())
    });
    f(adapter, "ExtractFileDrive", &[EMPTY], |value, args| {
        *value = text_value(extract_file_drive(&arg_text(args, 0)?).into_bytes());
        Ok(())
    });
    f(adapter, "ExtractFileName", &[EMPTY], |value, args| {
        *value = text_value(delphi::path_file_name(&arg_text(args, 0)?).as_bytes().to_vec());
        Ok(())
    });
    f(adapter, "ExtractFileExt", &[EMPTY], |value, args| {
        *value = text_value(extract_file_ext(&arg_text(args, 0)?).into_bytes());
        Ok(())
    });
    f(adapter, "ExpandFileName", &[EMPTY], |value, args| {
        *value = text_value(expand_file_name(&arg_text(args, 0)?).into_bytes());
        Ok(())
    });
    f(adapter, "ExpandUNCFileName", &[EMPTY], |value, args| {
        // A drive path is mapped to its share through the network API the
        // port does not call; the expanded name is the answer it gives.
        *value = text_value(expand_file_name(&arg_text(args, 0)?).into_bytes());
        Ok(())
    });
    f(adapter, "ExtractRelativePath", &[EMPTY, EMPTY], |value, args| {
        *value = text_value(extract_relative_path(&arg_text(args, 0)?, &arg_text(args, 1)?).into_bytes());
        Ok(())
    });
    f(adapter, "FileSearch", &[EMPTY, EMPTY], |value, args| {
        let name = arg_text(args, 0)?;
        let dirs = arg_text(args, 1)?;
        *value = text_value(file_search(&name, &dirs).into_bytes());
        Ok(())
    });
    f(adapter, "DiskFree", &[EMPTY], |value, _args| {
        // No volume API: the RTL's invalid-drive answer.
        *value = Value::Integer(-1);
        Ok(())
    });
    f(adapter, "DiskSize", &[EMPTY], |value, _args| {
        *value = Value::Integer(-1);
        Ok(())
    });
    f(adapter, "FileDateToDateTime", &[EMPTY], |value, args| {
        *value = Value::Date(datetime::file_date_to_date_time(arg_i32(args, 0)?));
        Ok(())
    });
    f(adapter, "DateTimeToFileDate", &[EMPTY], |value, args| {
        *value = Value::Integer(datetime::date_time_to_file_date(arg_f64(args, 0)?));
        Ok(())
    });
    f(adapter, "GetCurrentDir", &[EMPTY], |value, _args| {
        *value = text_value(
            std::env::current_dir()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default()
                .into_bytes(),
        );
        Ok(())
    });
    f(adapter, "SetCurrentDir", &[EMPTY], |value, args| {
        let dir = arg_text(args, 0)?;
        *value = Value::Bool(std::env::set_current_dir(dir).is_ok());
        Ok(())
    });
    f(adapter, "CreateDir", &[EMPTY], |value, args| {
        let dir = arg_text(args, 0)?;
        *value = Value::Bool(std::fs::create_dir(&dir).is_ok());
        Ok(())
    });
    f(adapter, "RemoveDir", &[EMPTY], |value, args| {
        let dir = arg_text(args, 0)?;
        *value = Value::Bool(std::fs::remove_dir(&dir).is_ok());
        Ok(())
    });

    //=== Dates and times ====================================================

    f(adapter, "EncodeDate", &[EMPTY, EMPTY, EMPTY], |value, args| {
        *value = Value::Double(
            datetime::encode_date(arg_i64(args, 0)?, arg_i64(args, 1)?, arg_i64(args, 2)?).map_err(external)?,
        );
        Ok(())
    });
    f(adapter, "EncodeTime", &[EMPTY, EMPTY, EMPTY, EMPTY], |value, args| {
        *value = Value::Double(
            datetime::encode_time(
                arg_i64(args, 0)?,
                arg_i64(args, 1)?,
                arg_i64(args, 2)?,
                arg_i64(args, 3)?,
            )
            .map_err(external)?,
        );
        Ok(())
    });
    f(adapter, "DecodeDate", &[EMPTY, BYREF, BYREF, BYREF], |_value, args| {
        let (year, month, day) = datetime::decode_date(arg_f64(args, 0)?);
        args.values[1] = Value::Integer(year as i32);
        args.values[2] = Value::Integer(month as i32);
        args.values[3] = Value::Integer(day as i32);
        Ok(())
    });
    f(
        adapter,
        "DecodeTime",
        &[EMPTY, BYREF, BYREF, BYREF, BYREF],
        |_value, args| {
            let (hour, minute, second, millisecond) = datetime::decode_time(arg_f64(args, 0)?);
            args.values[1] = Value::Integer(hour as i32);
            args.values[2] = Value::Integer(minute as i32);
            args.values[3] = Value::Integer(second as i32);
            args.values[4] = Value::Integer(millisecond as i32);
            Ok(())
        },
    );
    f(adapter, "DayOfWeek", &[EMPTY], |value, args| {
        *value = Value::Integer(datetime::day_of_week(arg_f64(args, 0)?) as i32);
        Ok(())
    });
    f(adapter, "Date", &[EMPTY], |value, _args| {
        *value = Value::Date(datetime::today());
        Ok(())
    });
    f(adapter, "Time", &[EMPTY], |value, _args| {
        *value = Value::Date(datetime::time_now());
        Ok(())
    });
    f(adapter, "Now", &[], |value, _args| {
        *value = Value::Date(datetime::now());
        Ok(())
    });
    f(adapter, "IncMonth", &[EMPTY, EMPTY], |value, args| {
        *value = Value::Date(datetime::inc_month(arg_f64(args, 0)?, arg_i64(args, 1)?));
        Ok(())
    });
    f(adapter, "IsLeapYear", &[EMPTY], |value, args| {
        *value = Value::Bool(datetime::is_leap_year(arg_i64(args, 0)?));
        Ok(())
    });
    f(adapter, "DateToStr", &[EMPTY], |value, args| {
        *value = text_value(datetime::date_to_str(arg_f64(args, 0)?).into_bytes());
        Ok(())
    });
    f(adapter, "TimeToStr", &[EMPTY], |value, args| {
        *value = text_value(datetime::time_to_str(arg_f64(args, 0)?).into_bytes());
        Ok(())
    });
    f(adapter, "DateTimeToStr", &[EMPTY], |value, args| {
        let format = format!(
            "{} {}",
            datetime::SETTINGS.short_date_format,
            datetime::SETTINGS.long_time_format
        );
        *value = text_value(datetime::date_time_to_str(&format, arg_f64(args, 0)?).into_bytes());
        Ok(())
    });
    f(adapter, "StrToDate", &[EMPTY], |value, args| {
        let text = arg_text(args, 0)?;
        *value = Value::Date(datetime::str_to_date(&text).map_err(external)?);
        Ok(())
    });
    f(adapter, "StrToTime", &[EMPTY], |value, args| {
        let text = arg_text(args, 0)?;
        *value = Value::Date(datetime::str_to_time(&text).map_err(external)?);
        Ok(())
    });
    f(adapter, "StrToDateTime", &[EMPTY], |value, args| {
        let text = arg_text(args, 0)?;
        *value = Value::Date(datetime::str_to_date_time(&text).map_err(external)?);
        Ok(())
    });
    f(adapter, "FormatDateTime", &[EMPTY, EMPTY], |value, args| {
        let format = String::from_utf8_lossy(&arg_str(args, 0)?).into_owned();
        *value = text_value(datetime::date_time_to_str(&format, arg_f64(args, 1)?).into_bytes());
        Ok(())
    });
    f(adapter, "DateTimeToString", &[BYREF, EMPTY, EMPTY], |value, args| {
        let format = String::from_utf8_lossy(&arg_str(args, 1)?).into_owned();
        let text = datetime::date_time_to_str(&format, arg_f64(args, 2)?);
        args.values[0] = Value::Str(text.clone().into_bytes());
        *value = Value::Str(text.into_bytes());
        Ok(())
    });

    //=== The locale and the system ==========================================

    f(adapter, "SysErrorMessage", &[EMPTY], |value, args| {
        let code = arg_i32(args, 0)?;
        // `FormatMessage` through the OS error text; code 0 is `''`.
        *value = text_value(if code == 0 {
            Vec::new()
        } else {
            std::io::Error::from_raw_os_error(code).to_string().into_bytes()
        });
        Ok(())
    });
    f(adapter, "GetLocaleStr", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let locale_type = arg_i32(args, 1)?;
        let default = arg_text(args, 2)?;
        *value = text_value(locale_string(locale_type).unwrap_or(default).into_bytes());
        Ok(())
    });
    f(adapter, "GetLocaleChar", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let locale_type = arg_i32(args, 1)?;
        let default = arg_text(args, 2)?;
        let text = locale_string(locale_type).unwrap_or(default);
        *value = text_value(text.into_bytes().into_iter().take(1).collect::<Vec<u8>>());
        Ok(())
    });
    f(adapter, "GetFormatSettings", &[], |_value, _args| {
        // The port's format settings are fixed at the RTL defaults; there is
        // nothing to reload from the OS.
        Ok(())
    });
    f(adapter, "ExceptObject", &[EMPTY], |value, _args| {
        // The `except on E` handler binds the caught exception as a script
        // variable; an adapter call cannot see it, so this is the nil
        // object the RTL returns outside a handler.
        *value = Value::Null;
        Ok(())
    });
    f(adapter, "ExceptAddr", &[EMPTY], |value, _args| {
        *value = Value::Pointer(0);
        Ok(())
    });
    f(
        adapter,
        "ExceptionErrorMessage",
        &[EMPTY, EMPTY, BYREF, EMPTY],
        |value, args| {
            // `Format(SExceptionErrorMessage, [E.ClassName, ModuleName,
            // Addr, E.Message])`: the port builds the same shape with the
            // object's class name and message and an empty module name.
            let (class, message) = match arg(args, 0).as_object() {
                Some(object) => {
                    let borrowed = object.borrow();
                    match borrowed.as_any().downcast_ref::<ExceptionObject>() {
                        Some(exception) => (borrowed.class().name.clone(), exception.message.clone()),
                        None => (borrowed.class().name.clone(), String::new()),
                    }
                }
                None => (String::new(), String::new()),
            };
            let address = arg_i64(args, 1).unwrap_or(0);
            let text = format!("Exception {class} in module  at {address:08X}.{message}");
            args.values[2] = Value::Str(text.clone().into_bytes());
            *value = Value::Integer(text.len() as i32 + 1);
            Ok(())
        },
    );
    f(adapter, "ShowException", &[EMPTY, EMPTY], |_value, _args| {
        // The RTL puts up a message box; a headless run has none, so the
        // call has nothing it can do.
        Ok(())
    });
    f(adapter, "Abort", &[], |_value, _args| {
        Err(ValueError::external_class("EAbort", S_ABORT_ERROR.to_owned()))
    });
    f(adapter, "OutOfMemoryError", &[], |_value, _args| {
        Err(external(S_OUT_OF_MEMORY))
    });
    f(adapter, "Beep", &[], |_value, _args| {
        // `MessageBeep(0)`; a hidden run must stay silent.
        Ok(())
    });

    //=== The MBCS helpers (single-byte code pages, as the corpus is) ========

    f(adapter, "ByteType", &[EMPTY, EMPTY], |value, _args| {
        // `TMbcsByteType`: `mbSingleByte` for the single-byte code pages.
        *value = Value::Integer(0);
        Ok(())
    });
    f(adapter, "StrByteType", &[EMPTY, EMPTY], |value, _args| {
        *value = Value::Integer(0);
        Ok(())
    });
    f(adapter, "ByteToCharLen", &[EMPTY, EMPTY], |value, args| {
        let text = arg_str(args, 0)?;
        let max = arg_i32(args, 1)?.max(0) as usize;
        *value = Value::Integer(max.min(text.len()) as i32);
        Ok(())
    });
    f(adapter, "CharToByteLen", &[EMPTY, EMPTY], |value, args| {
        let text = arg_str(args, 0)?;
        let max = arg_i32(args, 1)?.max(0) as usize;
        *value = Value::Integer(max.min(text.len()) as i32);
        Ok(())
    });
    f(adapter, "ByteToCharIndex", &[EMPTY, EMPTY], |value, args| {
        *value = Value::Integer(arg_i32(args, 1)?);
        Ok(())
    });
    f(adapter, "CharToByteIndex", &[EMPTY, EMPTY], |value, args| {
        *value = Value::Integer(arg_i32(args, 1)?);
        Ok(())
    });
    f(adapter, "IsPathDelimiter", &[EMPTY, EMPTY], |value, args| {
        let text = arg_str(args, 0)?;
        let index = arg_i32(args, 1)?;
        let delimiter =
            index >= 1 && (index as usize) <= text.len() && matches!(text[index as usize - 1], b'\\' | b'/');
        *value = Value::Bool(delimiter);
        Ok(())
    });
    f(adapter, "IsDelimiter", &[EMPTY, EMPTY, EMPTY], |value, args| {
        let delimiters = arg_str(args, 0)?;
        let text = arg_str(args, 1)?;
        let index = arg_i32(args, 2)?;
        let is_delimiter = index >= 1
            && (index as usize) <= text.len()
            && text[index as usize - 1] != 0
            && delimiters.contains(&text[index as usize - 1]);
        *value = Value::Bool(is_delimiter);
        Ok(())
    });
    f(adapter, "LastDelimiter", &[EMPTY, EMPTY], |value, args| {
        let delimiters = arg_str(args, 0)?;
        let text = arg_str(args, 1)?;
        let found = text
            .iter()
            .rposition(|byte| *byte != 0 && delimiters.contains(byte))
            .map(|index| index as i32 + 1)
            .unwrap_or(0);
        *value = Value::Integer(found);
        Ok(())
    });
    f(adapter, "AnsiCompareFileName", &[EMPTY, EMPTY], |value, args| {
        let a = String::from_utf8_lossy(&arg_str(args, 0)?).into_owned();
        let b = String::from_utf8_lossy(&arg_str(args, 1)?).into_owned();
        *value = Value::Integer(match ansi_compare_text(&a, &b) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        });
        Ok(())
    });
    f(adapter, "AnsiLowerCaseFileName", &[EMPTY], |value, args| {
        *value = text_value(lower_case(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "AnsiUpperCaseFileName", &[EMPTY], |value, args| {
        *value = text_value(upper_case(&arg_str(args, 0)?));
        Ok(())
    });
    f(adapter, "AnsiPos", &[EMPTY, EMPTY], |value, args| {
        let needle = arg_str(args, 0)?;
        let haystack = arg_str(args, 1)?;
        *value = Value::Integer(if needle.is_empty() {
            1
        } else {
            match haystack.windows(needle.len()).position(|window| window == needle) {
                Some(index) => index as i32 + 1,
                None => 0,
            }
        });
        Ok(())
    });
    f(adapter, "AnsiStrPos", &[EMPTY, EMPTY], |value, args| {
        let haystack = arg_str(args, 0)?;
        let needle = arg_str(args, 1)?;
        *value = text_value(pos(&needle, &haystack));
        Ok(())
    });
    f(adapter, "AnsiStrRScan", &[EMPTY, EMPTY], |value, args| {
        let text = arg_str(args, 0)?;
        let ch = arg_char(args, 1);
        *value = text_value(scan(&text, ch, true));
        Ok(())
    });
    f(adapter, "AnsiStrScan", &[EMPTY, EMPTY], |value, args| {
        let text = arg_str(args, 0)?;
        let ch = arg_char(args, 1);
        *value = text_value(scan(&text, ch, false));
        Ok(())
    });

    //=== Packages and the OS ================================================

    f(adapter, "LoadPackage", &[EMPTY], |_value, _args| {
        // The port has no Delphi package list; upstream loads a BPL.
        Err(ValueError::not_implemented("LoadPackage"))
    });
    f(adapter, "UnloadPackage", &[EMPTY], |_value, _args| {
        Err(ValueError::not_implemented("UnloadPackage"))
    });
    f(adapter, "RaiseLastWin32Error", &[], |_value, _args| {
        Err(external(os_error_message()))
    });
    f(adapter, "Win32Check", &[EMPTY], |value, args| {
        let result = arg_bool(args, 0)?;
        if !result {
            return Err(external(os_error_message()));
        }
        *value = Value::Bool(true);
        Ok(())
    });
}

/// `RaiseLastOSError`'s message: `GetLastError` through the last OS error.
fn os_error_message() -> String {
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) => format!("System Error. Code: {code}. {}", error),
        None => error.to_string(),
    }
}

/// The number of `TSearchRec` fields the find helpers touch.
const _: () = assert!(SR_FIND_HANDLE == 5);

/// Opens a handle and stores it in the registry.
fn store_file(file: std::fs::File) -> i32 {
    let handle = NEXT_FILE.with(|cell| {
        let handle = cell.get();
        cell.set(handle + 1);
        handle
    });
    FILES.with(|files| files.borrow_mut().insert(handle, file));
    handle
}

/// `FileSetDate`: sets the modified time from a TDateTime.
fn set_file_date(path: &str, date: f64) -> Option<i32> {
    let file = std::fs::OpenOptions::new().write(true).open(path).ok()?;
    let seconds = (date - datetime::UNIX_EPOCH_TDATE) * 86400.0;
    let modified = if seconds >= 0.0 {
        std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(seconds)
    } else {
        std::time::UNIX_EPOCH - std::time::Duration::from_secs_f64(-seconds)
    };
    let times = std::fs::FileTimes::new().set_modified(modified);
    file.set_times(times).ok()?;
    Some(0)
}

/// The payload of a value as raw bytes, the closest the port has to the
/// `TVarData` memory an upstream `varEmpty` parameter points at.
fn values_payload(value: &Value) -> Vec<u8> {
    match value {
        Value::Str(bytes) => bytes.clone(),
        Value::Integer(v) => v.to_le_bytes().to_vec(),
        other => xedit_script::values::to_str(other).unwrap_or_default(),
    }
}

/// `UpperCase` (the Unicode string one): the RTL maps only the ASCII
/// letters; the parity #39 probe showed `UpperCase(#228 + 'abc')` is
/// `#228ABC` in the oracle, where the ANSI variants map the code page's
/// high bytes too (`AnsiUpperCase` of `#228` is `#196`, through
/// `LCMapStringA`).
fn upper_case(text: &[u8]) -> Vec<u8> {
    text.iter().map(|byte| byte.to_ascii_uppercase()).collect()
}

fn lower_case(text: &[u8]) -> Vec<u8> {
    text.iter().map(|byte| byte.to_ascii_lowercase()).collect()
}

/// `CompareStr`/`CompareText`/their length-limited forms.
fn compare(a: &[u8], b: &[u8], case_insensitive: bool, max: Option<usize>) -> i32 {
    let (a, b) = match max {
        Some(max) => (&a[..a.len().min(max)], &b[..b.len().min(max)]),
        None => (a, b),
    };
    let count = a.len().min(b.len());
    for index in 0..count {
        let (ca, cb) = if case_insensitive {
            (a[index].to_ascii_uppercase(), b[index].to_ascii_uppercase())
        } else {
            (a[index], b[index])
        };
        if ca != cb {
            return i32::from(ca) - i32::from(cb);
        }
    }
    a.len() as i32 - b.len() as i32
}

/// `Trim`/`TrimLeft`/`TrimRight`: the RTL strips ' ' and lower control
/// characters.
fn trim(text: &[u8]) -> Vec<u8> {
    trim_right(&trim_left(text))
}

fn trim_left(text: &[u8]) -> Vec<u8> {
    let start = text.iter().position(|byte| *byte > b' ').unwrap_or(text.len());
    text[start..].to_vec()
}

fn trim_right(text: &[u8]) -> Vec<u8> {
    let end = text.iter().rposition(|byte| *byte > b' ').map(|at| at + 1).unwrap_or(0);
    text[..end].to_vec()
}

/// `QuotedStr`/`AnsiQuotedStr`.
fn quoted_str(text: &[u8], quote: u8) -> Vec<u8> {
    let mut out = vec![quote];
    for byte in text {
        out.push(*byte);
        if *byte == quote {
            out.push(quote);
        }
    }
    out.push(quote);
    out
}

/// `AnsiExtractQuotedStr`/`ExtractQuotedString`: the extracted text and the
/// remainder of the source.
fn extract_quoted(text: &[u8], quote: u8) -> (Vec<u8>, Vec<u8>) {
    if text.first() != Some(&quote) {
        return (Vec::new(), text.to_vec());
    }
    let mut out = Vec::new();
    let mut i = 1usize;
    while i < text.len() {
        if text[i] == quote {
            if text.get(i + 1) == Some(&quote) {
                out.push(quote);
                i += 2;
                continue;
            }
            return (out, text[i + 1..].to_vec());
        }
        out.push(text[i]);
        i += 1;
    }
    (out, Vec::new())
}

/// `AdjustLineBreaks`: a lone `#10` and a lone `#13` become `#13#10`.
fn adjust_line_breaks(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0usize;
    while i < text.len() {
        match text[i] {
            b'\r' => {
                out.extend_from_slice(b"\r\n");
                if text.get(i + 1) == Some(&b'\n') {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            b'\n' => {
                out.extend_from_slice(b"\r\n");
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    out
}

/// `IsValidIdent`.
fn is_valid_ident(text: &[u8]) -> bool {
    let Some(first) = text.first() else { return false };
    if !(first.is_ascii_alphabetic() || *first == b'_') {
        return false;
    }
    text[1..]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

/// The tail of a string from the first (or last) occurrence of `ch`,
/// `''` when it does not occur (the `PChar` result cast to a string).
fn scan(text: &[u8], ch: u8, reverse: bool) -> Vec<u8> {
    let found = if reverse {
        text.iter().rposition(|byte| *byte == ch)
    } else {
        text.iter().position(|byte| *byte == ch)
    };
    match found {
        Some(at) => text[at..].to_vec(),
        None => Vec::new(),
    }
}

/// The tail of `haystack` from the first occurrence of `needle` (the
/// `StrPos`/`AnsiStrPos` result).
fn pos(needle: &[u8], haystack: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return haystack.to_vec();
    }
    match haystack.windows(needle.len()).position(|window| window == needle) {
        Some(at) => haystack[at..].to_vec(),
        None => Vec::new(),
    }
}

/// `ExtractFileDir`: like `ExtractFilePath` without the trailing separator
/// (except for a root directory, which keeps it).
fn extract_file_dir(path: &str) -> String {
    let with_sep = delphi::extract_file_path(path);
    let trimmed = with_sep.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        return with_sep;
    }
    // `C:\` and `\\server\share\` keep their separator.
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        return with_sep;
    }
    trimmed.to_owned()
}

/// `ExtractFileDrive`.
fn extract_file_drive(path: &str) -> String {
    if let Some(at) = path.find(':') {
        return path[..=at].to_owned();
    }
    if let Some(rest) = path.strip_prefix("\\\\") {
        // `\\server\share` -- the drive of a UNC path is the share root.
        let mut parts = rest.splitn(3, '\\');
        if let (Some(server), Some(share)) = (parts.next(), parts.next()) {
            return format!("\\\\{server}\\{share}");
        }
    }
    String::new()
}

/// `ExtractFileExt`: the RTL scans the *whole* path for its last `.`
/// (`LastDelimiter('.' + PathDelim + DriveDelim, FileName)`), so a dot in a
/// directory name is part of the result; the port keeps that behaviour.
fn extract_file_ext(path: &str) -> String {
    match path.rfind('.') {
        Some(at) => path[at..].to_owned(),
        None => String::new(),
    }
}

/// `ExpandFileName`: an absolute path with `.` and `..` folded textually.
fn expand_file_name(path: &str) -> String {
    let absolute = if is_absolute_path(path) {
        path.to_owned()
    } else {
        let cwd = std::env::current_dir()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default();
        if cwd.is_empty() {
            path.to_owned()
        } else {
            format!("{}\\{path}", cwd.trim_end_matches(['\\', '/']))
        }
    };
    let (prefix, rest) = split_root(&absolute);
    fold_path(&prefix, rest)
}

/// The root of an absolute path: the drive (`C:`) or the UNC share
/// (`\\server\share`), and the remainder.
fn split_root(absolute: &str) -> (String, &str) {
    if absolute.len() >= 2 && absolute.as_bytes()[1] == b':' {
        return (absolute[..2].to_owned(), &absolute[2..]);
    }
    if let Some(unc) = absolute.strip_prefix("\\\\") {
        let mut parts = unc.splitn(3, '\\');
        if let (Some(server), Some(share)) = (parts.next(), parts.next()) {
            let prefix_len = 2 + server.len() + 1 + share.len();
            return (absolute[..prefix_len].to_owned(), &absolute[prefix_len..]);
        }
    }
    (String::new(), absolute)
}

/// The `.`/`..` folding of [`expand_file_name`].
fn fold_path(prefix: &str, rest: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split(['\\', '/']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("{prefix}\\{}", parts.join("\\"))
}

fn is_absolute_path(path: &str) -> bool {
    (path.len() >= 2 && path.as_bytes()[1] == b':') || path.starts_with('\\') || path.starts_with('/')
}

/// `ExtractRelativePath`: the destination relative to the base directory,
/// or the destination itself when the base does not prefix it.
fn extract_relative_path(base: &str, dest: &str) -> String {
    let base = base.replace('/', "\\");
    let dest_norm = dest.replace('/', "\\");
    if dest_norm.len() > base.len() && dest_norm[..base.len()].eq_ignore_ascii_case(&base) {
        dest_norm[base.len()..].trim_start_matches('\\').to_owned()
    } else {
        dest.to_owned()
    }
}

/// `FileSearch`: the first directory of the semicolon list that holds the
/// file, else `''`.
fn file_search(name: &str, dir_list: &str) -> String {
    for dir in dir_list.split(';') {
        let dir = if dir.is_empty() { ".".to_owned() } else { dir.to_owned() };
        let candidate = format!("{}\\{name}", dir.trim_end_matches(['\\', '/']));
        if Path::new(&candidate).is_file() {
            return candidate;
        }
    }
    String::new()
}

/// `GetLocaleStr` for the en-US locale: the RTL's `LOCALE_S*` values the
/// port can answer (the day and month names, the separators, the date and
/// time formats, the number symbols); an unknown type is `None`, for which
/// the caller uses the default.
fn locale_string(locale_type: i32) -> Option<String> {
    const LOCALE_SDECIMAL: i32 = 14;
    const LOCALE_STHOUSAND: i32 = 15;
    const LOCALE_SCURRENCY: i32 = 20;
    const LOCALE_SDATE: i32 = 29;
    const LOCALE_STIME: i32 = 30;
    const LOCALE_SSHORTDATE: i32 = 31;
    const LOCALE_SLONGDATE: i32 = 32;
    const LOCALE_S1159: i32 = 40;
    const LOCALE_S2359: i32 = 41;
    const LOCALE_SDAYNAME1: i32 = 42;
    const LOCALE_SABBREVDAYNAME1: i32 = 49;
    const LOCALE_SMONTHNAME1: i32 = 56;
    const LOCALE_SABBREVMONTHNAME1: i32 = 68;
    const LOCALE_STIMEFORMAT: i32 = 4099;
    Some(match locale_type {
        LOCALE_SDECIMAL => format::SETTINGS.decimal_separator.to_string(),
        LOCALE_STHOUSAND => format::SETTINGS.thousand_separator.to_string(),
        LOCALE_SCURRENCY => format::SETTINGS.currency_string.to_owned(),
        LOCALE_SDATE => datetime::SETTINGS.date_separator.to_string(),
        LOCALE_STIME => datetime::SETTINGS.time_separator.to_string(),
        LOCALE_SSHORTDATE => datetime::SETTINGS.short_date_format.to_owned(),
        LOCALE_SLONGDATE => "dddd, MMMM d, yyyy".to_owned(),
        LOCALE_STIMEFORMAT => "h:nn:ss am/pm".to_owned(),
        LOCALE_S1159 => datetime::SETTINGS.time_am_string.to_owned(),
        LOCALE_S2359 => datetime::SETTINGS.time_pm_string.to_owned(),
        // `LOCALE_SDAYNAME1`..`LOCALE_SDAYNAME7` (Monday first).
        42..=48 => datetime::SETTINGS.long_day_names[((locale_type - LOCALE_SDAYNAME1 + 1) % 7) as usize].to_owned(),
        // `LOCALE_SABBREVDAYNAME1`..`_7`.
        49..=55 => {
            datetime::SETTINGS.short_day_names[((locale_type - LOCALE_SABBREVDAYNAME1 + 1) % 7) as usize].to_owned()
        }
        // `LOCALE_SMONTHNAME1`..`_12`.
        56..=67 => datetime::SETTINGS.long_month_names[(locale_type - LOCALE_SMONTHNAME1) as usize].to_owned(),
        // `LOCALE_SABBREVMONTHNAME1`..`_12`.
        68..=79 => datetime::SETTINGS.short_month_names[(locale_type - LOCALE_SABBREVMONTHNAME1) as usize].to_owned(),
        _ => return None,
    })
}
