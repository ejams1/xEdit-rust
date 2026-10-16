// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterMiscVcl.pas

//! `xejviScriptAdapterMiscVcl.pas`, the misc family's VCL unit.
//!
//! What answers headless here: the `IniFiles` classes (`TCustomIniFile`,
//! `TIniFile`, `TMemIniFile`) over [`IniFileRef`], the `IOUtils` record types
//! `TDirectory` and `TFile` (the `GetDirectories`/`GetFiles` walk and the two
//! text-file helpers) and `THashedStringList.Create` (a `TStringList` of the
//! `Classes` unit's object model under this class name, since the list has no
//! window). The binary readers and writers of the `Classes` unit are
//! answered too (`script_rtl/classes.rs`: their object model is the `TStream`
//! family's, and none of them needs a window). Everything else -- the bitmap,
//! the check list box, combo box, labeled edits, list view items, menus and
//! `TWinControl` members, and `TRegistryIniFile` (the registry, not a file) --
//! needs a live object and goes through the [`Vcl`] seam, refusing headless
//! with the clear error.

use std::cell::RefCell;
use std::rc::Rc;

use xedit_script::adapter::{Adapter, AdapterGetFn, Args};
use xedit_script::values::{ClassDef, ScriptObject, Value, ValueError, VarType, VariantArray, to_bool, to_i32, to_str};

use super::{MemberKind, Vcl, class_of, member_handler, params, setter_handler};
use crate::script::ScriptRunState;
use crate::script_strings::StringListObject;
use crate::script_units::{strings_add, strings_at};

/// The control classes this unit registers (the ini-file and string-list
/// classes are answered here, not by the object model).
pub(crate) const CONTROL_CLASSES: &[&str] = &["TBoundLabel", "TCustomLabeledEdit", "TLabeledEdit", "TCheckListBox"];

/// Registers the unit: the twelve classes, the five constants, the `IOUtils`
/// record types with their methods, the `IniFiles` classes and the members
/// that go through the seam.
pub(crate) fn register(adapter: &mut Adapter, _state: &Rc<RefCell<ScriptRunState>>, vcl: &Rc<RefCell<dyn Vcl>>) {
    let t_custom_label = class_of(adapter, "StdCtrls", "TCustomLabel");
    let _t_bound_label = adapter.add_class("ExtCtrls", "TBoundLabel", Some(t_custom_label));
    let t_custom_edit = class_of(adapter, "StdCtrls", "TCustomEdit");
    let t_custom_labeled_edit = adapter.add_class("ExtCtrls", "TCustomLabeledEdit", Some(t_custom_edit));
    let t_labeled_edit = adapter.add_class("ExtCtrls", "TLabeledEdit", Some(t_custom_labeled_edit.clone()));
    let t_custom_list_box = class_of(adapter, "StdCtrls", "TCustomListBox");
    let t_check_list_box = adapter.add_class("CheckLst", "TCheckListBox", Some(t_custom_list_box));
    let t_object_again = class_of(adapter, "Classes", "TObject");
    let t_custom_ini_file = adapter.add_class("IniFiles", "TCustomIniFile", Some(t_object_again));
    let t_ini_file = adapter.add_class("IniFiles", "TIniFile", Some(t_custom_ini_file.clone()));
    let t_mem_ini_file = adapter.add_class("IniFiles", "TMemIniFile", Some(t_custom_ini_file.clone()));
    let t_registry_ini_file = adapter.add_class("Registry", "TRegistryIniFile", Some(t_custom_ini_file.clone()));
    let t_string_list = class_of(adapter, "Classes", "TStringList");
    let t_hashed_string_list = adapter.add_class("IniFiles", "THashedStringList", Some(t_string_list));

    for (name, value) in [("seBorder", 2), ("seClient", 1), ("seFont", 0)] {
        adapter.add_const("Controls", name, Value::Integer(value));
    }
    for (name, value) in [("soAllDirectories", 1), ("soTopDirectoryOnly", 0)] {
        adapter.add_const("IOUtils", name, Value::Integer(value));
    }

    // The members whose handler is the seam: the controls and the
    // registry-backed ini file.
    for (class, name, kind, count, encoded) in SHIM_MEMBERS {
        let class_def = class_of(adapter, "", class);
        let params = params(encoded);
        let count = *count;
        match kind {
            'g' => adapter.add_get(
                class_def,
                name,
                member_handler(vcl, class, name, MemberKind::Get),
                count,
                params,
                VarType::Empty,
            ),
            's' => adapter.add_set(
                class_def,
                name,
                setter_handler(vcl, class, name, MemberKind::Set),
                count,
                params,
            ),
            'i' => adapter.add_iget(
                class_def,
                name,
                member_handler(vcl, class, name, MemberKind::IndexedGet),
                count,
                params,
                VarType::Empty,
            ),
            'I' => adapter.add_iset(
                class_def,
                name,
                setter_handler(vcl, class, name, MemberKind::IndexedSet),
                count,
                params,
            ),
            _ => unreachable!(),
        }
    }
    let _ = (&t_registry_ini_file, &t_labeled_edit, &t_check_list_box, &t_ini_file);

    // `THashedStringList.Create`: a `TStringList` object of the `Classes`
    // unit's model under this class name.
    {
        let class = t_hashed_string_list.clone();
        adapter.add_get(
            t_hashed_string_list,
            "Create",
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                let list = StringListObject::new(class.clone());
                *value = Value::Object(Rc::new(RefCell::new(list)) as Rc<RefCell<dyn ScriptObject>>);
                Ok(())
            }) as AdapterGetFn,
            0,
            params(""),
            VarType::Empty,
        );
    }

    register_ini_members(adapter, &t_custom_ini_file);
    register_ini_creates(adapter, &t_ini_file, &t_mem_ini_file);

    // `IOUtils`: the two record types and their methods.
    let t_directory = adapter.add_rec("IOUtils", "TDirectory", 0, Vec::new());
    let t_file = adapter.add_rec("IOUtils", "TFile", 0, Vec::new());
    register_io_members(adapter, &t_directory, &t_file);

    for (unit, event, event_class) in [
        ("CheckLst", "TNotifyEvent", "TJvInterpreterCheckListBoxEvents"),
        ("ComCtrls", "TLVOwnerDataEvent", "TJvInterpreterListViewEvents"),
        ("ComCtrls", "TLVSelectItemEvent", "TJvInterpreterListViewEvents"),
    ] {
        adapter.add_handler(unit, event, event_class);
    }
}

/// The members of the unit that need a live object: (class, member, kind,
/// count, params). The binary stream classes of the `Classes` unit are not
/// here: they are ordinary stream operations and are registered with that
/// unit (`crates/xedit-session/src/script_rtl/classes.rs`).
const SHIM_MEMBERS: &[(&str, &str, char, i32, &str)] = &[
    // TBitmap (`SetSize`)
    ("TBitmap", "SetSize", 'g', 2, ".."),
    // TCheckListBox
    ("TCheckListBox", "AllowGrayed", 'g', 0, "."),
    ("TCheckListBox", "AllowGrayed", 's', 0, "."),
    ("TCheckListBox", "CheckAll", 'g', 3, "..."),
    ("TCheckListBox", "Checked", 'i', 1, "."),
    ("TCheckListBox", "Checked", 'I', 1, "n"),
    ("TCheckListBox", "Create", 'g', 1, "."),
    ("TCheckListBox", "Header", 'i', 1, "."),
    ("TCheckListBox", "Header", 'I', 1, "n"),
    ("TCheckListBox", "ItemEnabled", 'i', 1, "."),
    ("TCheckListBox", "ItemEnabled", 'I', 1, "n"),
    ("TCheckListBox", "State", 'i', 1, "."),
    ("TCheckListBox", "State", 'I', 1, "n"),
    // TComboBox
    ("TComboBox", "DropDownCount", 'g', 0, "."),
    ("TComboBox", "DropDownCount", 's', 0, "."),
    // TControl
    ("TControl", "ScaleValue", 'g', 1, "."),
    ("TControl", "StyleElements", 'g', 0, "."),
    ("TControl", "StyleElements", 's', 0, "."),
    // TCustomForm
    ("TCustomForm", "PopupMode", 'g', 0, "."),
    ("TCustomForm", "PopupMode", 's', 0, "."),
    ("TCustomForm", "PopupParent", 'g', 0, "."),
    ("TCustomForm", "PopupParent", 's', 0, "."),
    // TCustomLabeledEdit
    ("TCustomLabeledEdit", "Create", 'g', 1, "."),
    ("TCustomLabeledEdit", "EditLabel", 'g', 0, "."),
    ("TCustomLabeledEdit", "LabelPosition", 'g', 0, "."),
    ("TCustomLabeledEdit", "LabelPosition", 's', 0, "."),
    ("TCustomLabeledEdit", "LabelSpacing", 'g', 0, "."),
    ("TCustomLabeledEdit", "LabelSpacing", 's', 0, "."),
    // TLabeledEdit
    ("TLabeledEdit", "Create", 'g', 1, "."),
    // TListItem / TListItems
    ("TListItem", "SubItems", 'g', 0, "."),
    ("TListItem", "SubItems", 's', 0, "."),
    ("TListItems", "Count", 's', 0, "."),
    // TMenu / TMenuItem
    ("TMenu", "AutoHotKeys", 'g', 0, "."),
    ("TMenu", "AutoHotKeys", 's', 0, "."),
    ("TMenuItem", "Clear", 'g', 0, ""),
    // TRegistryIniFile
    ("TRegistryIniFile", "Create", 'g', 1, "."),
    // TWinControl
    ("TWinControl", "DoubleBuffered", 'g', 0, "."),
    ("TWinControl", "DoubleBuffered", 's', 0, "."),
];

/// `TCustomIniFile`'s members: the `IniFiles` unit's methods over
/// [`IniFileRef`]. `Read*`/`SectionExists`/`ValueExists` answer; the
/// `Write*`/`DeleteKey`/`EraseSection`/`UpdateFile` change the object.
fn register_ini_members(adapter: &mut Adapter, class: &Rc<ClassDef>) {
    let get = |adapter: &mut Adapter, name: &str, count: i32, encoded: &str, func: AdapterGetFn| {
        adapter.add_get(class.clone(), name, func, count, params(encoded), VarType::Empty);
    };
    get(adapter, "DeleteKey", 2, "..", Rc::new(ini_delete_key));
    get(adapter, "EraseSection", 1, ".", Rc::new(ini_erase_section));
    get(adapter, "ReadBool", 3, "...", Rc::new(ini_read_bool));
    get(adapter, "ReadFloat", 3, "...", Rc::new(ini_read_float));
    get(adapter, "ReadInteger", 3, "...", Rc::new(ini_read_integer));
    get(adapter, "ReadSection", 2, "..", Rc::new(ini_read_section));
    get(adapter, "ReadSections", 1, ".", Rc::new(ini_read_sections));
    get(adapter, "ReadSectionValues", 2, "..", Rc::new(ini_read_section_values));
    get(adapter, "ReadString", 3, "...", Rc::new(ini_read_string));
    get(adapter, "SectionExists", 1, ".", Rc::new(ini_section_exists));
    get(adapter, "UpdateFile", 0, "", Rc::new(ini_update_file));
    get(adapter, "ValueExists", 2, "..", Rc::new(ini_value_exists));
    get(adapter, "WriteBool", 3, "...", Rc::new(ini_write_bool));
    get(adapter, "WriteFloat", 3, "...", Rc::new(ini_write_float));
    get(adapter, "WriteInteger", 3, "...", Rc::new(ini_write_integer));
    get(adapter, "WriteString", 3, "...", Rc::new(ini_write_string));
}

/// `TIniFile.Create`/`TMemIniFile.Create(FileName)` and the two
/// `TMemIniFile` string-list methods.
fn register_ini_creates(adapter: &mut Adapter, t_ini_file: &Rc<ClassDef>, t_mem_ini_file: &Rc<ClassDef>) {
    {
        // A `TIniFile` writes through on every change; a `TMemIniFile` keeps
        // the content in memory until `UpdateFile`.
        let class = t_ini_file.clone();
        adapter.add_get(
            t_ini_file.clone(),
            "Create",
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let path = arg_string(args, 0)?;
                let object = IniFileRef::load(class.clone(), path, true);
                *value = Value::Object(Rc::new(RefCell::new(object)) as Rc<RefCell<dyn ScriptObject>>);
                Ok(())
            }) as AdapterGetFn,
            1,
            params("."),
            VarType::Empty,
        );
    }
    {
        let class = t_mem_ini_file.clone();
        adapter.add_get(
            t_mem_ini_file.clone(),
            "Create",
            Rc::new(move |value: &mut Value, args: &mut Args| {
                let path = arg_string(args, 0)?;
                let object = IniFileRef::load(class.clone(), path, false);
                *value = Value::Object(Rc::new(RefCell::new(object)) as Rc<RefCell<dyn ScriptObject>>);
                Ok(())
            }) as AdapterGetFn,
            1,
            params("."),
            VarType::Empty,
        );
    }
    adapter.add_get(
        t_mem_ini_file.clone(),
        "GetStrings",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let lines = ini_object(&args.obj)?.borrow().to_lines();
            strings_add(args, 0, lines.into_iter().map(String::into_bytes).collect());
            Ok(())
        }),
        1,
        params("."),
        VarType::Empty,
    );
    adapter.add_get(
        t_mem_ini_file.clone(),
        "SetStrings",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let Some(object) = strings_at(args, 0) else {
                return Ok(());
            };
            let lines: Vec<String> = {
                let borrowed = object.borrow();
                let Some(list) = borrowed.string_list() else {
                    return Ok(());
                };
                list.lines()
                    .iter()
                    .map(|line| String::from_utf8_lossy(line).into_owned())
                    .collect()
            };
            ini_object(&args.obj)?.borrow_mut().set_lines(&lines)
        }),
        1,
        params("."),
        VarType::Empty,
    );
}

/// The path of the exception's message: `IOUtils` normalizes it, so a run
/// of separators collapses to one (`M:\...\Data\\Textures\` prints as
/// `M:\...\Data\Textures\` in the oracle's log).
fn collapsed_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut last = '\0';
    for c in path.chars() {
        if c == '\\' && last == '\\' {
            continue;
        }
        last = c;
        out.push(c);
    }
    out
}

/// `IOUtils`: `TDirectory.GetDirectories`/`GetFiles` and
/// `TFile.ReadAllText`/`WriteAllText`.
fn register_io_members(
    adapter: &mut Adapter,
    t_directory: &Rc<xedit_script::values::RecordDef>,
    t_file: &Rc<xedit_script::values::RecordDef>,
) {
    adapter.add_rec_get(
        t_directory.clone(),
        "GetDirectories",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let path = arg_string(args, 0)?;
            let pattern = arg_string(args, 1)?;
            let all = to_i32(&args.values[2])? != 0;
            if !std::path::Path::new(&path).is_dir() {
                // `TDirectory.GetDirectories` of IOUtils raises for a path
                // that is not there (`EDirectoryNotFoundException` of
                // `Directory.Exists`; the message is the system text of
                // `ERROR_PATH_NOT_FOUND`, and the path in brackets on the
                // next line of the message).
                return Err(ValueError::external_class(
                    "EDirectoryNotFoundException",
                    format!("The specified path was not found\n[{}]", collapsed_path(&path)),
                ));
            }
            *value = string_array(directories(&path, &pattern, all));
            Ok(())
        }),
        3,
        params("..."),
        VarType::Empty,
    );
    adapter.add_rec_get(
        t_directory.clone(),
        "GetFiles",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let path = arg_string(args, 0)?;
            let pattern = arg_string(args, 1)?;
            let all = to_i32(&args.values[2])? != 0;
            if !std::path::Path::new(&path).is_dir() {
                // `DDS - Find textures without mipmaps.pas` relies on it: the
                // oracle aborts on its private data folder without `Textures`
                // (`Exception in unit DDSFindTexturesNoMipMaps line 16:
                // [EDirectoryNotFoundException] The specified path was not
                // found`), and the message carries the path in brackets on
                // its second line (the oracle log's next line is
                // `[<path>]`).
                return Err(ValueError::external_class(
                    "EDirectoryNotFoundException",
                    format!("The specified path was not found\n[{}]", collapsed_path(&path)),
                ));
            }
            *value = string_array(files(&path, &pattern, all));
            Ok(())
        }),
        3,
        params("..."),
        VarType::Empty,
    );
    adapter.add_rec_get(
        t_file.clone(),
        "ReadAllText",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let path = arg_string(args, 0)?;
            match std::fs::read(&path) {
                Ok(bytes) => {
                    // `TFile.ReadAllText` detects the encoding; the port hands
                    // the bytes to the script as an ANSI string (a UTF-8 BOM
                    // stays, as reading the file through `TStringList` would
                    // keep it).
                    *value = Value::Str(bytes);
                    Ok(())
                }
                Err(error) => Err(ValueError::external_class(
                    "EFOpenError",
                    format!(
                        "Cannot open file \"{path}\". {}",
                        crate::script_strings::os_error_text(&error)
                    ),
                )),
            }
        }),
        1,
        params("."),
        VarType::Empty,
    );
    adapter.add_rec_get(
        t_file.clone(),
        "WriteAllText",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let path = arg_string(args, 0)?;
            let text = to_str(&args.values[1])?;
            std::fs::write(&path, &text).map_err(|error| {
                ValueError::external_class(
                    "EFCreateError",
                    format!(
                        "Cannot create file \"{path}\". {}",
                        crate::script_strings::os_error_text(&error)
                    ),
                )
            })
        }),
        2,
        params(".."),
        VarType::Empty,
    );
}

/// A `TArray<string>` result (`TDirectory.GetFiles`): a variant array of
/// strings (`nil` upstream for a folder that does not exist -- the port
/// returns the empty array, which `Length` reads as 0).
fn string_array(paths: Vec<String>) -> Value {
    Value::VariantArray(Rc::new(RefCell::new(VariantArray::of_strings(
        paths.into_iter().map(|path| Value::Str(path.into_bytes())).collect(),
    ))))
}

/// `TDirectory.GetDirectories(Path, Pattern, SearchOption)`: full paths.
fn directories(path: &str, pattern: &str, all: bool) -> Vec<String> {
    let mut found = Vec::new();
    collect_entries(path, pattern, all, true, &mut found);
    found
}

/// `TDirectory.GetFiles(Path, Pattern, SearchOption)`: full paths, each
/// directory's files before its subdirectories' (`SearchAllDirs`).
fn files(path: &str, pattern: &str, all: bool) -> Vec<String> {
    let mut found = Vec::new();
    collect_entries(path, pattern, all, false, &mut found);
    found
}

fn collect_entries(path: &str, pattern: &str, all: bool, want_dirs: bool, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    let mut subdirectories = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().map(|typ| typ.is_dir()).unwrap_or(false);
        let full = entry.path().to_string_lossy().into_owned();
        if is_dir {
            subdirectories.push(full.clone());
            if want_dirs && matches_pattern(&name, pattern) {
                found.push(full);
            }
        } else if !want_dirs && matches_pattern(&name, pattern) {
            found.push(full);
        }
    }
    if all {
        for subdirectory in subdirectories {
            collect_entries(&subdirectory, pattern, all, want_dirs, found);
        }
    }
}

/// The wildcard match of `System.Masks` (`*`, `?`, `[abc]`, `[a-z]`,
/// `[!...]`), case-insensitively.
fn matches_pattern(name: &str, pattern: &str) -> bool {
    fn matches(name: &[u8], pattern: &[u8]) -> bool {
        match pattern.split_first() {
            None => name.is_empty(),
            Some((b'*', rest)) => (0..=name.len()).any(|skip| matches(&name[skip..], rest)),
            Some((b'?', rest)) => !name.is_empty() && matches(&name[1..], rest),
            Some((b'[', rest)) => {
                let Some(end) = rest.iter().position(|byte| *byte == b']') else {
                    // An unterminated class is a literal `[`.
                    return !name.is_empty() && name[0] == b'[' && matches(&name[1..], rest);
                };
                let (class, rest) = (&rest[..end], &rest[end + 1..]);
                let (negated, class) = match class.split_first() {
                    Some((b'!', class)) => (true, class),
                    _ => (false, class),
                };
                let Some(first) = name.first() else {
                    return false;
                };
                let in_class = class_in(first.to_ascii_uppercase(), class);
                if in_class == negated {
                    return false;
                }
                matches(&name[1..], rest)
            }
            Some((expected, rest)) => {
                !name.is_empty() && name[0].eq_ignore_ascii_case(expected) && matches(&name[1..], rest)
            }
        }
    }

    fn class_in(byte: u8, class: &[u8]) -> bool {
        let mut at = 0;
        while at < class.len() {
            if at + 2 < class.len() && class[at + 1] == b'-' {
                let (low, high) = (class[at].to_ascii_uppercase(), class[at + 2].to_ascii_uppercase());
                if low <= byte && byte <= high {
                    return true;
                }
                at += 3;
            } else {
                if class[at].to_ascii_uppercase() == byte {
                    return true;
                }
                at += 1;
            }
        }
        false
    }

    matches(name.as_bytes(), pattern.as_bytes())
}

//=== Argument access ========================================================

/// The [`IniFileRef`] behind a `TCustomIniFile` receiver.
fn ini_object(obj: &Option<Value>) -> Result<Rc<RefCell<IniFileState>>, ValueError> {
    let invalid = || ValueError::external_class("EInvalidCast", "the receiver is not a TCustomIniFile".to_owned());
    let Some(Value::Object(object)) = obj else {
        return Err(invalid());
    };
    let state = object
        .borrow()
        .as_any()
        .downcast_ref::<IniFileRef>()
        .map(|reference| reference.state.clone());
    state.ok_or_else(invalid)
}

/// `string(Args.Values[i])`.
fn arg_string(args: &Args, index: usize) -> Result<String, ValueError> {
    Ok(String::from_utf8_lossy(&to_str(&args.values[index])?).into_owned())
}

/// The `Strings.Clear` the `ReadSection*` members do before they fill a
/// `TStrings` (`TCustomIniFile.ReadSections` and friends), then the lines.
fn strings_replace(args: &Args, index: usize, lines: Vec<String>) {
    let Some(object) = strings_at(args, index) else {
        return;
    };
    if let Some(list) = object.borrow_mut().string_list_mut() {
        list.clear();
        for line in lines {
            list.add(line.into_bytes());
        }
    }
}

/// `Boolean(Args.Values[i])`.
fn arg_bool(args: &Args, index: usize) -> Result<bool, ValueError> {
    to_bool(&args.values[index])
}

//=== The IniFiles members ===================================================

fn ini_delete_key(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let name = arg_string(args, 1)?;
    ini_object(&args.obj)?.borrow_mut().delete_key(&section, &name)
}

fn ini_erase_section(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    ini_object(&args.obj)?.borrow_mut().erase_section(&section)
}

fn ini_read_bool(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let name = arg_string(args, 1)?;
    let default = arg_bool(args, 2)?;
    let read = ini_object(&args.obj)?.borrow().read_bool(&section, &name, default);
    *value = Value::Bool(read);
    Ok(())
}

fn ini_read_float(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let name = arg_string(args, 1)?;
    let default = xedit_script::values::to_f64(&args.values[2])?;
    let read = ini_object(&args.obj)?.borrow().read_float(&section, &name, default)?;
    *value = Value::Double(read);
    Ok(())
}

fn ini_read_integer(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let name = arg_string(args, 1)?;
    let default = to_i32(&args.values[2])?;
    let read = ini_object(&args.obj)?.borrow().read_integer(&section, &name, default);
    *value = Value::Integer(read);
    Ok(())
}

fn ini_read_section(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let names = ini_object(&args.obj)?.borrow().section_names(&section);
    strings_replace(args, 1, names);
    Ok(())
}

fn ini_read_sections(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let sections = ini_object(&args.obj)?.borrow().sections();
    strings_replace(args, 0, sections);
    Ok(())
}

fn ini_read_section_values(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let values = ini_object(&args.obj)?.borrow().section_values(&section);
    strings_replace(args, 1, values);
    Ok(())
}

fn ini_read_string(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let name = arg_string(args, 1)?;
    let default = arg_string(args, 2)?;
    let read = ini_object(&args.obj)?.borrow().read_string(&section, &name, &default);
    *value = Value::Str(read.into_bytes());
    Ok(())
}

fn ini_section_exists(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let exists = ini_object(&args.obj)?.borrow().section_exists(&section);
    *value = Value::Bool(exists);
    Ok(())
}

fn ini_update_file(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    ini_object(&args.obj)?.borrow_mut().update_file()
}

fn ini_value_exists(value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let name = arg_string(args, 1)?;
    let exists = ini_object(&args.obj)?.borrow().value_exists(&section, &name);
    *value = Value::Bool(exists);
    Ok(())
}

fn ini_write_bool(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let name = arg_string(args, 1)?;
    let value = arg_bool(args, 2)?;
    ini_object(&args.obj)?
        .borrow_mut()
        .write(&section, &name, &bool_text(value))
}

fn ini_write_float(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let name = arg_string(args, 1)?;
    let value = xedit_script::values::to_f64(&args.values[2])?;
    ini_object(&args.obj)?
        .borrow_mut()
        .write(&section, &name, &xedit_core::delphi::float_to_str(value))
}

fn ini_write_integer(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let name = arg_string(args, 1)?;
    let value = to_i32(&args.values[2])?;
    ini_object(&args.obj)?
        .borrow_mut()
        .write(&section, &name, &value.to_string())
}

fn ini_write_string(_value: &mut Value, args: &mut Args) -> Result<(), ValueError> {
    let section = arg_string(args, 0)?;
    let name = arg_string(args, 1)?;
    let value = arg_string(args, 2)?;
    ini_object(&args.obj)?.borrow_mut().write(&section, &name, &value)
}

/// `WriteBool`'s text: `IntToStr(Ord(Value))` -- `1` and `0`, which
/// `ReadInteger`/`ReadBool` read back (captured from the oracle: a
/// `WriteBool(True)` gives `C=1` in the file).
fn bool_text(value: bool) -> String {
    if value { "1".to_owned() } else { "0".to_owned() }
}

//=== The TCustomIniFile object ==============================================

/// The state of a `TCustomIniFile`: the sections and their name/value pairs,
/// read from the file at `Create` and written back by `UpdateFile` (a
/// `TIniFile` writes through on every change instead).
///
/// The parse rules are `System.IniFiles`': a line before the first section is
/// dropped, `;` starts a comment, names and values are trimmed, sections and
/// names compare without case and the first of a repeated one wins -- the
/// same rules the phase 5 Sniff port's settings reader uses
/// (`xedit_assets::sniff::processor`).
pub struct IniFileState {
    file_name: String,
    sections: Vec<(String, Vec<(String, String)>)>,
    /// A `TIniFile` (write-through) rather than a `TMemIniFile`.
    auto_write: bool,
}

/// The script-visible object: `TCustomIniFile` and its descendants hold an
/// [`IniFileState`] behind a shared handle, so the members can borrow it.
struct IniFileRef {
    class: Rc<ClassDef>,
    state: Rc<RefCell<IniFileState>>,
}

impl IniFileRef {
    fn load(class: Rc<ClassDef>, file_name: String, auto_write: bool) -> IniFileRef {
        let text = std::fs::read(&file_name)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        IniFileRef {
            class,
            state: Rc::new(RefCell::new(IniFileState {
                file_name,
                sections: text.as_deref().map(parse_sections).unwrap_or_default(),
                auto_write,
            })),
        }
    }
}

impl ScriptObject for IniFileRef {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl IniFileState {
    fn section(&self, section: &str) -> Option<&Vec<(String, String)>> {
        self.sections
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(section))
            .map(|(_, values)| values)
    }

    fn value_exists(&self, section: &str, name: &str) -> bool {
        self.section(section)
            .is_some_and(|values| values.iter().any(|(key, _)| key.eq_ignore_ascii_case(name)))
    }

    fn section_exists(&self, section: &str) -> bool {
        self.section(section).is_some()
    }

    fn read_string(&self, section: &str, name: &str, default: &str) -> String {
        self.section(section)
            .and_then(|values| values.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)))
            .map_or_else(|| default.to_owned(), |(_, value)| value.clone())
    }

    /// `ReadInteger`: `0X` prefixed values are hexadecimal, a value that is
    /// not a number gives the default.
    fn read_integer(&self, section: &str, name: &str, default: i32) -> i32 {
        let mut text = self.read_string(section, name, "");
        if text.len() > 2 && text[..2].eq_ignore_ascii_case("0x") {
            text = format!("${}", &text[2..]);
        }
        str_to_int_default(&text, default)
    }

    /// `ReadBool`: `ReadInteger(...) <> 0`.
    fn read_bool(&self, section: &str, name: &str, default: bool) -> bool {
        self.read_integer(section, name, i32::from(default)) != 0
    }

    /// `ReadFloat`: the text through `StrToFloat` (a text that is not a
    /// number raises `EConvertError`; an absent value gives the default).
    fn read_float(&self, section: &str, name: &str, default: f64) -> Result<f64, ValueError> {
        let text = self.read_string(section, name, "");
        if text.is_empty() {
            return Ok(default);
        }
        xedit_core::delphi::str_to_float(&text).ok_or_else(|| {
            ValueError::external_class("EConvertError", format!("'{text}' is not a valid floating point value"))
        })
    }

    /// `ReadSections`: every section name, in file order.
    fn sections(&self) -> Vec<String> {
        self.sections.iter().map(|(name, _)| name.clone()).collect()
    }

    /// `ReadSection`: the *names* of a section, in file order.
    fn section_names(&self, section: &str) -> Vec<String> {
        self.section(section)
            .map(|values| values.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default()
    }

    /// `ReadSectionValues`: `Name=Value` lines.
    fn section_values(&self, section: &str) -> Vec<String> {
        self.section(section)
            .map(|values| values.iter().map(|(name, value)| format!("{name}={value}")).collect())
            .unwrap_or_default()
    }

    /// `TMemIniFile.GetStrings`: each section with its values and an empty
    /// line after it.
    fn to_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (section, values) in &self.sections {
            lines.push(format!("[{section}]"));
            for (name, value) in values {
                lines.push(format!("{name}={value}"));
            }
            lines.push(String::new());
        }
        lines
    }

    /// `SetStrings`: replace the content with the given lines (a `TIniFile`
    /// writes through, as it does for every change).
    fn set_lines(&mut self, lines: &[String]) -> Result<(), ValueError> {
        self.sections = parse_sections(&lines.join("\r\n"));
        self.after_change()
    }

    fn write(&mut self, section: &str, name: &str, value: &str) -> Result<(), ValueError> {
        self.set_string(section, name, value);
        self.after_change()
    }

    fn set_string(&mut self, section: &str, name: &str, value: &str) {
        if self
            .sections
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(section))
            .is_none()
        {
            self.sections.push((section.to_owned(), Vec::new()));
        }
        let target = &mut self
            .sections
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(section))
            .expect("just added")
            .1;
        match target.iter_mut().find(|(key, _)| key.eq_ignore_ascii_case(name)) {
            Some((_, slot)) => *slot = value.to_owned(),
            None => target.push((name.to_owned(), value.to_owned())),
        }
    }

    fn delete_key(&mut self, section: &str, name: &str) -> Result<(), ValueError> {
        if let Some(values) = self
            .sections
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(section))
            .map(|(_, values)| values)
        {
            values.retain(|(key, _)| !key.eq_ignore_ascii_case(name));
        }
        self.after_change()
    }

    fn erase_section(&mut self, section: &str) -> Result<(), ValueError> {
        self.sections.retain(|(name, _)| !name.eq_ignore_ascii_case(section));
        self.after_change()
    }

    /// `UpdateFile`: write the object to its file.
    fn update_file(&mut self) -> Result<(), ValueError> {
        let text = self.to_file_text();
        std::fs::write(&self.file_name, text).map_err(|error| {
            ValueError::external_class(
                "EFCreateError",
                format!(
                    "Cannot create file \"{}\". {}",
                    self.file_name,
                    crate::script_strings::os_error_text(&error)
                ),
            )
        })
    }

    /// A `TIniFile` writes through on every change; a `TMemIniFile` waits for
    /// `UpdateFile`.
    fn after_change(&mut self) -> Result<(), ValueError> {
        if self.auto_write {
            self.update_file()?;
        }
        Ok(())
    }

    /// `TMemIniFile.UpdateFile`'s text: each section with its values and an
    /// empty line after it, with `CRLF`.
    fn to_file_text(&self) -> String {
        let mut text = String::new();
        for (section, values) in &self.sections {
            text.push_str(&format!("[{section}]\r\n"));
            for (name, value) in values {
                text.push_str(&format!("{name}={value}\r\n"));
            }
            text.push_str("\r\n");
        }
        text
    }
}

/// `StrToIntDef`: an integer, else the default.
fn str_to_int_default(text: &str, default: i32) -> i32 {
    let trimmed = text.trim();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let value = match digits.strip_prefix('$') {
        Some(hex) if !hex.is_empty() => i64::from_str_radix(hex, 16).ok(),
        Some(_) => None,
        None => digits.parse::<i64>().ok(),
    };
    match value {
        Some(value) if !negative => i32::try_from(value).unwrap_or(default),
        Some(value) => i32::try_from(-value).unwrap_or(default),
        None => default,
    }
}

/// `TMemIniFile.SetStrings`/`TStringList` parsing (`System.IniFiles`).
fn parse_sections(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut current: Option<usize> = None;
    for line in text.split(['\r', '\n']) {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.len() >= 2 && line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].trim().to_owned();
            sections.push((name, Vec::new()));
            current = Some(sections.len() - 1);
        } else if let Some(section) = current {
            let (name, value) = match line.find('=') {
                Some(index) => (line[..index].trim().to_owned(), line[index + 1..].trim().to_owned()),
                None => (line.to_owned(), String::new()),
            };
            sections[section].1.push((name, value));
        }
    }
    sections
}

/// The port-side ledger keys of this unit, built the way
/// `crates/xtask/src/main.rs` builds them (`script_registrations`).
#[cfg(test)]
pub(crate) fn ledger_keys() -> std::collections::BTreeSet<String> {
    let mut keys = std::collections::BTreeSet::new();
    let mut add = |key: String| {
        keys.insert(key);
    };
    for (unit, name) in [
        ("Classes", "TBinaryReader"),
        ("Classes", "TBinaryWriter"),
        ("Classes", "TBytesStream"),
        ("ExtCtrls", "TBoundLabel"),
        ("ExtCtrls", "TCustomLabeledEdit"),
        ("ExtCtrls", "TLabeledEdit"),
        ("CheckLst", "TCheckListBox"),
        ("IniFiles", "TCustomIniFile"),
        ("IniFiles", "TIniFile"),
        ("IniFiles", "TMemIniFile"),
        ("Registry", "TRegistryIniFile"),
        ("IniFiles", "THashedStringList"),
    ] {
        add(format!("xejviScriptAdapterMiscVcl:{unit}.{name}:class"));
    }
    for (unit, name) in [
        ("Controls", "seBorder"),
        ("Controls", "seClient"),
        ("Controls", "seFont"),
        ("IOUtils", "soAllDirectories"),
        ("IOUtils", "soTopDirectoryOnly"),
    ] {
        add(format!("xejviScriptAdapterMiscVcl:{unit}.{name}:const"));
    }
    for (class, member, kind, _, _) in SHIM_MEMBERS {
        let kind = match kind {
            'g' => "get",
            's' => "set",
            'i' => "iget",
            'I' => "iset",
            _ => unreachable!(),
        };
        add(format!("xejviScriptAdapterMiscVcl:{class}.{member}:{kind}"));
    }
    // The three binary stream classes of this unit: their members are bound
    // with the `Classes` unit's stream object model
    // (`crate::script_rtl::classes::register_binary_streams`), so they are
    // not rows of [`SHIM_MEMBERS`].
    for (class, member) in [
        ("TBinaryReader", "Create"),
        ("TBinaryReader", "Read"),
        ("TBinaryReader", "ReadBoolean"),
        ("TBinaryReader", "ReadByte"),
        ("TBinaryReader", "ReadBytes"),
        ("TBinaryReader", "ReadChar"),
        ("TBinaryReader", "ReadDouble"),
        ("TBinaryReader", "ReadInteger"),
        ("TBinaryReader", "ReadShortInt"),
        ("TBinaryReader", "ReadSingle"),
        ("TBinaryReader", "ReadSmallInt"),
        ("TBinaryReader", "ReadString"),
        ("TBinaryReader", "ReadUInt16"),
        ("TBinaryReader", "ReadUInt32"),
        ("TBinaryWriter", "Create"),
        ("TBinaryWriter", "Write"),
        ("TBinaryWriter", "WriteSingle"),
        ("TBytesStream", "Create"),
    ] {
        add(format!("xejviScriptAdapterMiscVcl:{class}.{member}:get"));
    }
    for (class, member, kind) in [
        ("TIniFile", "Create", "get"),
        ("TMemIniFile", "Create", "get"),
        ("TMemIniFile", "GetStrings", "get"),
        ("TMemIniFile", "SetStrings", "get"),
        ("THashedStringList", "Create", "get"),
        ("TCustomIniFile", "DeleteKey", "get"),
        ("TCustomIniFile", "EraseSection", "get"),
        ("TCustomIniFile", "ReadBool", "get"),
        ("TCustomIniFile", "ReadFloat", "get"),
        ("TCustomIniFile", "ReadInteger", "get"),
        ("TCustomIniFile", "ReadSection", "get"),
        ("TCustomIniFile", "ReadSections", "get"),
        ("TCustomIniFile", "ReadSectionValues", "get"),
        ("TCustomIniFile", "ReadString", "get"),
        ("TCustomIniFile", "SectionExists", "get"),
        ("TCustomIniFile", "UpdateFile", "get"),
        ("TCustomIniFile", "ValueExists", "get"),
        ("TCustomIniFile", "WriteBool", "get"),
        ("TCustomIniFile", "WriteFloat", "get"),
        ("TCustomIniFile", "WriteInteger", "get"),
        ("TCustomIniFile", "WriteString", "get"),
    ] {
        add(format!("xejviScriptAdapterMiscVcl:{class}.{member}:{kind}"));
    }
    for (record, member) in [
        ("TDirectory", "GetDirectories"),
        ("TDirectory", "GetFiles"),
        ("TFile", "ReadAllText"),
        ("TFile", "WriteAllText"),
    ] {
        add(format!("xejviScriptAdapterMiscVcl:{record}.{member}:recget"));
    }
    for (unit, event) in [
        ("CheckLst", "TNotifyEvent"),
        ("ComCtrls", "TLVOwnerDataEvent"),
        ("ComCtrls", "TLVSelectItemEvent"),
    ] {
        add(format!("xejviScriptAdapterMiscVcl:{unit}.{event}:handler"));
    }
    for record in ["TDirectory", "TFile"] {
        add(format!("xejviScriptAdapterMiscVcl:IOUtils.{record}:rec"));
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ini_file_state_reads_and_writes_delphi_s_ini_text() {
        // `TMemIniFile.SetStrings` / the file's parse rules: comments,
        // trimmed names and values, case-insensitive sections and names, the
        // first of a repeated one wins.
        let text = "; comment\r\n\r\n[SECTION]\r\nName = Value \r\nNumber=7\r\nHex=0X10\r\nFlag=1\r\nDup=first\r\nDup=second\r\n\r\n[Other]\r\nX=1\r\n";
        let sections = parse_sections(text);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].0, "SECTION");
        assert_eq!(sections[0].1[0], ("Name".to_owned(), "Value".to_owned()));
        assert_eq!(sections[0].1.len(), 6);
        let mut state = IniFileState {
            file_name: String::new(),
            sections,
            auto_write: false,
        };
        assert_eq!(state.read_string("section", "name", "d"), "Value");
        assert_eq!(state.read_string("SECTION", "missing", "d"), "d");
        assert_eq!(state.read_integer("Section", "Number", 0), 7);
        assert_eq!(state.read_integer("Section", "Hex", 0), 16);
        assert_eq!(state.read_integer("Section", "Text", 9), 9);
        assert!(state.read_bool("Section", "Flag", false));
        assert!(state.read_bool("Section", "Number", false));
        assert!(state.value_exists("Section", "name"));
        assert!(state.section_exists("section"));
        assert!(!state.section_exists("Third"));
        assert_eq!(state.sections(), vec!["SECTION".to_owned(), "Other".to_owned()]);
        assert_eq!(state.section_names("SECTION")[0], "Name");
        assert_eq!(state.section_values("Other"), vec!["X=1".to_owned()]);
        // `WriteString`/`WriteInteger`/`WriteBool` change the sections; the
        // `UpdateFile` text is the sections with an empty line after each.
        state.write("New", "K", "v").unwrap();
        state.write("SECTION", "Name", "changed").unwrap();
        state.write("SECTION", "Bool", &bool_text(true)).unwrap();
        assert_eq!(
            state.to_file_text(),
            "[SECTION]\r\nName=changed\r\nNumber=7\r\nHex=0X10\r\nFlag=1\r\nDup=first\r\nDup=second\r\nBool=1\r\n\r\n[Other]\r\nX=1\r\n\r\n[New]\r\nK=v\r\n\r\n"
        );
        state.delete_key("Section", "number").unwrap();
        assert!(!state.value_exists("SECTION", "Number"));
        state.erase_section("other").unwrap();
        assert!(!state.section_exists("Other"));
    }

    /// A script's `WriteFloat` text is `FloatToStr`, and a text the value of
    /// which is not a number raises the `EConvertError` `ReadFloat` raises.
    #[test]
    fn the_ini_file_state_reads_floats_like_delphi() {
        let mut sections = parse_sections("[S]\r\nE=1.5\r\nBad=abc\r\n");
        sections.push(("T".to_owned(), Vec::new()));
        let state = IniFileState {
            file_name: String::new(),
            sections,
            auto_write: false,
        };
        assert_eq!(state.read_float("S", "E", 0.0).unwrap(), 1.5);
        assert_eq!(state.read_float("S", "Missing", 2.5).unwrap(), 2.5);
        assert_eq!(state.read_float("T", "E", 0.0).unwrap(), 0.0);
        let error = state.read_float("S", "Bad", 0.0).unwrap_err();
        match error {
            ValueError::ExternalClass { class, message } => {
                assert_eq!(class, "EConvertError");
                assert!(
                    message.contains("'abc' is not a valid floating point value"),
                    "{message}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    /// `TDirectory.GetFiles`/`GetDirectories`: full paths, the `*`/`?`
    /// pattern (case-insensitive), a directory's own entries before its
    /// subdirectories' and nothing when the `soTopDirectoryOnly` option is 0.
    #[test]
    fn the_directory_walk_matches_delphi_s() {
        let folder = std::env::temp_dir().join(format!("xedit-vcl-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("sub").join("deep")).unwrap();
        std::fs::write(folder.join("a.pas"), "a").unwrap();
        std::fs::write(folder.join("b.txt"), "b").unwrap();
        std::fs::write(folder.join("sub").join("c.PAS"), "c").unwrap();
        std::fs::write(folder.join("sub").join("deep").join("d.pas"), "d").unwrap();

        let path = folder.to_string_lossy().into_owned();
        let found = files(&path, "*.pas", false);
        assert_eq!(found, vec![folder.join("a.pas").to_string_lossy().into_owned()]);
        let found = files(&path, "*.pas", true);
        assert_eq!(
            found,
            vec![
                folder.join("a.pas").to_string_lossy().into_owned(),
                folder.join("sub").join("c.PAS").to_string_lossy().into_owned(),
                folder
                    .join("sub")
                    .join("deep")
                    .join("d.pas")
                    .to_string_lossy()
                    .into_owned(),
            ]
        );
        // The character class of `System.Masks` is case-insensitive too.
        let found = files(&path, "?.[tp]as", true);
        assert_eq!(found.len(), 3);
        let found = files(&path, "?.pas", false);
        assert_eq!(found.len(), 1);
        let found = files(&path, "*", false);
        assert_eq!(found.len(), 2);
        let directories = directories(&path, "*", true);
        assert_eq!(
            directories,
            vec![
                folder.join("sub").to_string_lossy().into_owned(),
                folder.join("sub").join("deep").to_string_lossy().into_owned(),
            ]
        );
        assert!(files(&folder.join("missing").to_string_lossy(), "*", true).is_empty());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
