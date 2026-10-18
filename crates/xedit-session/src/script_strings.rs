// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_Classes.pas (the
// `TStrings` and `TStringList` object model and the base methods the phase 6
// step 7 slice registers: Create, Add, AddStrings, Append, Assign, Clear,
// Count, Delete, Insert, IndexOf, Exchange, Text, Strings[], Sort, Find,
// Duplicates and Sorted) with Delphi's System.Classes `TStringList`
// semantics (CompareStrings, Find, Add with Sorted/Duplicates, Get/SetText,
// Get/SetDelimitedText, AnsiExtractQuotedStr, LoadFromFile/SaveToFile) and
// the `TEncoding` object of xejviScriptAdapterMisc.pas over xedit-io's
// encoding module.

//! The `TStrings`/`TStringList` script object and the `TEncoding` objects of
//! the Misc family.
//!
//! The phase 6 step 7 registrations of `xejviScriptAdapterMisc.pas` work on
//! `TStrings` objects (`wbFilterStrings`, `wbRemoveDuplicateStrings`,
//! `GetRecordDefNames`, `Delimiter`, `DelimitedText`, `ValueFromIndex`,
//! `CaseSensitive`, `AddPair`, `Contains*`, `LoadFromFile`, `SaveToFile`,
//! `SetStrings`, the four set operations) and on `TEncoding` objects
//! (`LoadFromFile`/`SaveToFile` with an encoding). No other step has
//! registered the two classes yet -- they belong to the JVCL `Classes` unit
//! of step 8 -- so this module carries the object model and the *base*
//! method slice those registrations need. Step 8 extends the slice with the
//! rest of `JvInterpreter_Classes.pas`; it must not register a second
//! `TStrings`/`TStringList` class (see `script_classes.rs`).
//!
//! `TStrings` has no separate abstract object here: every object a script
//! can hold is created by `TStringList.Create`, so the `TStrings` class is
//! only the method home, as upstream.

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use xedit_io::collate::{ansi_compare_str, ansi_compare_text};
use xedit_io::encoding::{ansi_bytes, ansi_string, string_list_text};

use xedit_script::adapter::{Adapter, Args};
use xedit_script::strings::{Duplicates, StringList as SharedStringList};
use xedit_script::values::{ClassDef, ParamType, ScriptObject, Value, ValueError, VarType};

use crate::script_classes::ScriptClasses;

/// `TDuplicates` (`System.Classes`): what `Add` does with a duplicate of a
/// sorted list. No `AddConst` registers the names for scripts; a script
/// writes `sl.Duplicates := 1`.
pub const DUP_IGNORE: i32 = 0;
pub const DUP_ACCEPT: i32 = 1;
pub const DUP_ERROR: i32 = 2;

/// `EStringListError` of the RTL, the class of a string list error.
pub const STRING_LIST_ERROR_CLASS: &str = "EStringListError";
/// `SSortedListError` (`RTLConsts`).
pub const SORTED_LIST_ERROR: &str = "Operation not allowed on sorted list";
/// `SDuplicateString` (`RTLConsts`).
pub const DUPLICATE_STRING_ERROR: &str = "String list does not allow duplicates";

/// One `TStringList` (which is also the port's `TStrings`).
///
/// The lines are bytes, as the shared [`xedit_script::strings::StringList`]
/// the step-5 and step-6 host functions fill keeps them, and the object
/// implements that trait, so a `TStrings` parameter they write is the same
/// list this surface reads and writes (`Value::string_list_mut`). The
/// text-facing methods convert lossily at the boundary.
pub struct StringListObject {
    class: Rc<ClassDef>,
    strings: Vec<Vec<u8>>,
    /// `Objects[]`: what `AddObject`/`InsertObject` put beside a line; an
    /// `Empty` is no object (upstream's nil).
    objects: Vec<Value>,
    sorted: bool,
    duplicates: i32,
    case_sensitive: bool,
    delimiter: char,
    strict_delimiter: bool,
    name_value_separator: char,
    /// A read past the last line is `''` instead of `EStringListError`: the
    /// `Lines` of a `TRichEdit` (`TRichEditStrings.Get` asks the control,
    /// which answers nothing for a line it does not have).
    lenient_index: bool,
}

impl StringListObject {
    pub fn new(class: Rc<ClassDef>) -> StringListObject {
        StringListObject {
            class,
            strings: Vec::new(),
            objects: Vec::new(),
            sorted: false,
            duplicates: DUP_IGNORE,
            case_sensitive: false,
            delimiter: ',',
            strict_delimiter: false,
            name_value_separator: '=',
            lenient_index: false,
        }
    }

    /// Marks the list as one whose out-of-range reads are `''` (the
    /// `TRichEdit.Lines` of the interactive controls).
    pub fn set_lenient_index(&mut self, lenient: bool) {
        self.lenient_index = lenient;
    }

    /// `CompareStrings` (`System.Classes`): `AnsiCompareText`, or
    /// `AnsiCompareStr` for a case-sensitive list.
    fn compare(&self, a: &str, b: &str) -> std::cmp::Ordering {
        if self.case_sensitive {
            ansi_compare_str(a, b)
        } else {
            ansi_compare_text(a, b)
        }
    }

    pub fn count(&self) -> usize {
        self.strings.len()
    }

    /// The lines as text (lossily), in order -- what the set operations and
    /// the helpers walk.
    pub fn strings(&self) -> Vec<String> {
        self.strings.iter().map(|line| line_text(line)).collect()
    }

    pub fn line_bytes(&self, index: usize) -> Option<&[u8]> {
        self.strings.get(index).map(Vec::as_slice)
    }

    pub fn string_at(&self, index: i64) -> String {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.strings.get(index))
            .map(|line| line_text(line))
            .unwrap_or_default()
    }

    pub fn set_string(&mut self, index: i64, text: String) {
        if let Ok(index) = usize::try_from(index)
            && index < self.strings.len()
        {
            self.strings[index] = text.into_bytes();
        }
    }

    /// `TStringList.Find`: the binary search over the sorted order.
    /// UPSTREAM-QUIRK: Delphi runs it whatever `Sorted` says (the RTL has
    /// no unsorted fallback), so on an unsorted list the result is whatever
    /// the search lands on; the port reproduces the algorithm. `index` is
    /// the insertion point when the string is not there.
    pub fn find(&self, text: &str) -> (bool, usize) {
        let mut l: i64 = 0;
        let mut h: i64 = self.strings.len() as i64 - 1;
        let mut found = false;
        while l <= h {
            let i = (l + h) >> 1;
            let c = self.compare(&self.string_at(i), text);
            if c == std::cmp::Ordering::Less {
                l = i + 1;
            } else {
                h = i - 1;
                if c == std::cmp::Ordering::Equal {
                    found = true;
                    if self.duplicates != DUP_ACCEPT {
                        l = i;
                    }
                }
            }
        }
        (found, l.max(0) as usize)
    }

    /// `TStringList.Add`: the index the entry was added at, or the index of
    /// the duplicate an ignored one finds.
    pub fn add(&mut self, text: String) -> Result<usize, ValueError> {
        self.add_bytes(text.into_bytes())
    }

    /// [`StringListObject::add`] for a byte line (the shared trait's shape).
    pub fn add_bytes(&mut self, text: Vec<u8>) -> Result<usize, ValueError> {
        let index = if !self.sorted {
            self.strings.len()
        } else {
            let (found, index) = self.find(&line_text(&text));
            if found {
                match self.duplicates {
                    DUP_IGNORE => return Ok(index),
                    DUP_ERROR => {
                        return Err(ValueError::external_class(
                            STRING_LIST_ERROR_CLASS,
                            DUPLICATE_STRING_ERROR.to_owned(),
                        ));
                    }
                    _ => {}
                }
            }
            index
        };
        self.insert_item(index, text);
        Ok(index)
    }

    /// `TStrings.Append` -- `Add` without the index.
    pub fn append(&mut self, text: String) -> Result<(), ValueError> {
        self.add(text).map(|_| ())
    }

    fn insert_item(&mut self, index: usize, text: Vec<u8>) {
        let index = index.min(self.strings.len());
        self.strings.insert(index, text);
        self.objects.insert(index, Value::Empty);
    }

    /// `TStrings.Insert`: refused on a sorted list (`SSortedListError`).
    pub fn insert(&mut self, index: i64, text: String) -> Result<(), ValueError> {
        if self.sorted {
            return Err(ValueError::external_class(
                STRING_LIST_ERROR_CLASS,
                SORTED_LIST_ERROR.to_owned(),
            ));
        }
        // `Insert` accepts `Count` itself as the index.
        self.check_index(index, self.strings.len() + 1)?;
        self.insert_item(index as usize, text.into_bytes());
        Ok(())
    }

    /// `TStrings.Error(@SListIndexError, Index)`: `List index out of bounds
    /// (%d).  TStringList range is %d..%d` (the RTL's two spaces), or
    /// `TStringList is empty` without entries (probes `x-008`, `x-043`,
    /// `x-044`).
    pub fn check_index(&self, index: i64, count: usize) -> Result<(), ValueError> {
        if index >= 0 && (index as usize) < count {
            return Ok(());
        }
        let message = if count == 0 {
            format!("List index out of bounds ({index}).  TStringList is empty")
        } else {
            format!(
                "List index out of bounds ({index}).  TStringList range is 0..{}",
                count - 1
            )
        };
        Err(ValueError::external_class(STRING_LIST_ERROR_CLASS, message))
    }

    /// `TStrings.Delete`.
    pub fn delete(&mut self, index: i64) -> Result<(), ValueError> {
        self.check_index(index, self.strings.len())?;
        self.strings.remove(index as usize);
        self.objects.remove(index as usize);
        Ok(())
    }

    /// `TStrings.Exchange`.
    pub fn exchange(&mut self, first: i64, second: i64) -> Result<(), ValueError> {
        self.check_index(first, self.strings.len())?;
        self.check_index(second, self.strings.len())?;
        self.strings.swap(first as usize, second as usize);
        self.objects.swap(first as usize, second as usize);
        Ok(())
    }

    /// `TStrings.IndexOf` (a linear search whatever `Sorted` says).
    pub fn index_of(&self, text: &str) -> i64 {
        self.strings
            .iter()
            .position(|entry| self.compare(&line_text(entry), text) == std::cmp::Ordering::Equal)
            .map_or(-1, |index| index as i64)
    }

    /// `TStrings.IndexOfName`: the first entry whose name part (before the
    /// name-value separator) matches, compared with `SameText`.
    pub fn index_of_name(&self, name: &str) -> i64 {
        self.strings
            .iter()
            .position(|entry| match line_text(entry).split_once(self.name_value_separator) {
                Some((entry_name, _)) => entry_name.eq_ignore_ascii_case(name),
                None => name.is_empty(),
            })
            .map_or(-1, |index| index as i64)
    }

    /// `TStrings.IndexOfObject`: the first entry whose object is the same
    /// object (pointer identity upstream).
    pub fn index_of_object(&self, object: &Value) -> i64 {
        let equal = |entry: &Value| -> bool {
            match (entry, object) {
                (Value::Empty | Value::Null, Value::Empty | Value::Null) => true,
                (Value::Object(a), Value::Object(b)) => xedit_script::values::same_object(a, b),
                _ => false,
            }
        };
        self.objects.iter().position(equal).map_or(-1, |index| index as i64)
    }

    pub fn object_at(&self, index: i64) -> Value {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.objects.get(index))
            .cloned()
            .unwrap_or(Value::Null)
    }

    pub fn set_object_at(&mut self, index: i64, object: Value) {
        if let Ok(index) = usize::try_from(index)
            && index < self.objects.len()
        {
            self.objects[index] = object;
        }
    }

    /// `TStringList.Sort` in `CompareStrings` order. UPSTREAM-QUIRK:
    /// upstream sorts with an unsteady QuickSort, the port stably, so
    /// entries that compare equal keep their order.
    pub fn sort(&mut self) {
        let mut entries: Vec<(Vec<u8>, Value)> = self.strings.drain(..).zip(self.objects.drain(..)).collect();
        entries.sort_by(|a, b| self.compare(&line_text(&a.0), &line_text(&b.0)));
        for (text, object) in entries {
            self.strings.push(text);
            self.objects.push(object);
        }
    }

    /// `TStringList.SetSorted`: a write of `True` sorts (the RTL sorts
    /// inside `SetSorted` before it sets the field).
    pub fn set_sorted(&mut self, sorted: bool) {
        if sorted && !self.sorted {
            self.sort();
        }
        self.sorted = sorted;
    }

    pub fn sorted(&self) -> bool {
        self.sorted
    }

    pub fn set_case_sensitive(&mut self, case_sensitive: bool) {
        if self.case_sensitive != case_sensitive {
            self.case_sensitive = case_sensitive;
            if self.sorted {
                self.sort();
            }
        }
    }

    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    pub fn set_duplicates(&mut self, duplicates: i32) {
        self.duplicates = duplicates;
    }

    pub fn duplicates(&self) -> i32 {
        self.duplicates
    }

    /// `TStrings.Text` (`GetTextStr`): the lines joined by `#13#10`, with a
    /// trailing break when there are any.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for entry in &self.strings {
            out.push_str(&line_text(entry));
            out.push_str("\r\n");
        }
        out
    }

    /// `TStrings.Text` (`SetTextStr`): split on `#13`, `#10` and `#13#10`;
    /// a trailing line break adds no empty line. Each line goes through
    /// `Add`, so a sorted list inserts in order and `Duplicates` applies
    /// (`System.Classes.TStrings.SetTextStr` calls `Add`).
    pub fn set_text(&mut self, text: &str) -> Result<(), ValueError> {
        self.strings.clear();
        self.objects.clear();
        let bytes = text.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            let start = index;
            while index < bytes.len() && bytes[index] != b'\r' && bytes[index] != b'\n' {
                index += 1;
            }
            self.add_bytes(text.as_bytes()[start..index].to_vec())?;
            if index < bytes.len() && bytes[index] == b'\r' {
                index += 1;
            }
            if index < bytes.len() && bytes[index] == b'\n' {
                index += 1;
            }
        }
        Ok(())
    }

    /// `TStrings.SetStrings`/`Assign`: the other list's lines and objects.
    pub fn assign(&mut self, other: &StringListObject) {
        self.strings = other.strings.clone();
        self.objects = other.objects.clone();
    }

    /// A whole copy, so a caller can compare two lists while holding only
    /// one of them borrowed.
    pub fn snapshot(&self) -> StringListObject {
        StringListObject {
            class: self.class.clone(),
            strings: self.strings.clone(),
            objects: self.objects.clone(),
            sorted: self.sorted,
            duplicates: self.duplicates,
            case_sensitive: self.case_sensitive,
            delimiter: self.delimiter,
            strict_delimiter: self.strict_delimiter,
            name_value_separator: self.name_value_separator,
            lenient_index: self.lenient_index,
        }
    }

    pub fn clear(&mut self) {
        self.strings.clear();
        self.objects.clear();
    }

    pub fn delimiter(&self) -> char {
        self.delimiter
    }

    pub fn set_delimiter(&mut self, delimiter: char) {
        self.delimiter = delimiter;
    }

    pub fn strict_delimiter(&self) -> bool {
        self.strict_delimiter
    }

    pub fn set_strict_delimiter(&mut self, strict: bool) {
        self.strict_delimiter = strict;
    }

    pub fn set_name_value_separator(&mut self, separator: char) {
        self.name_value_separator = separator;
    }

    /// `TStrings.ValueFromIndex[Index]`: the text after the name-value
    /// separator, or the whole entry when there is none.
    pub fn value_from_index(&self, index: i64) -> String {
        let entry = self.string_at(index);
        match entry.split_once(self.name_value_separator) {
            Some((_, value)) => value.to_owned(),
            None => entry,
        }
    }

    /// `TStrings.ValueFromIndex[Index] := Value`: replaces the text after
    /// the separator, or appends `NameValueSeparator + Value` when the
    /// entry has none.
    pub fn set_value_from_index(&mut self, index: i64, value: &str) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let separator = self.name_value_separator;
        let Some(entry) = self.strings.get_mut(index) else {
            return;
        };
        let text = line_text(entry);
        *entry = match text.split_once(separator) {
            Some((name, _)) => format!("{name}{separator}{value}"),
            None => format!("{text}{separator}{value}"),
        }
        .into_bytes();
    }

    /// `TStrings.GetDelimitedText` (`System.Classes`): the lines joined by
    /// `Delimiter`, every line that holds a control character, a space, the
    /// quote character or the delimiter given by `AnsiQuotedStr` style
    /// quoting. A single empty line reads `""`, as the RTL gives it. The
    /// RTL consults `StrictDelimiter` only when parsing, never when
    /// writing.
    pub fn delimited_text(&self) -> String {
        if self.strings.len() == 1 && self.strings[0].is_empty() {
            return "\"\"".to_owned();
        }
        let lines = self.strings();
        let mut out = String::new();
        for (index, entry) in lines.iter().enumerate() {
            if index > 0 {
                out.push(self.delimiter);
            }
            let needs_quotes = entry
                .chars()
                .any(|ch| (ch as u32) <= b' ' as u32 || ch == '"' || ch == self.delimiter);
            if needs_quotes {
                out.push('"');
                for ch in entry.chars() {
                    if ch == '"' {
                        out.push('"');
                    }
                    out.push(ch);
                }
                out.push('"');
            } else {
                out.push_str(entry);
            }
        }
        out
    }

    /// `TStrings.SetDelimitedText` (`System.Classes`): the parser of
    /// `DelimitedText`. Unless `StrictDelimiter` is set, a run of
    /// characters up to and including a space is skipped before each field
    /// and a field ends at the first such character; with it, only the
    /// delimiter ends a field. A quoted field goes through
    /// `AnsiExtractQuotedStr`, and two adjacent delimiters make an empty
    /// field.
    pub fn set_delimited_text(&mut self, text: &str) -> Result<(), ValueError> {
        self.set_delimited_text_with(text, self.delimiter)
    }

    /// `TStrings.CommaText := value`: the `SetDelimitedText` parser with
    /// `,` as the delimiter, whatever `Delimiter` holds
    /// (`TStrings.SetCommaText` passes the comma, not the property), and
    /// the list's `StrictDelimiter` deciding whether spaces also end a
    /// field.
    pub fn set_comma_text(&mut self, text: &str) -> Result<(), ValueError> {
        self.set_delimited_text_with(text, ',')
    }

    fn set_delimited_text_with(&mut self, text: &str, delimiter: char) -> Result<(), ValueError> {
        let chars: Vec<char> = text.chars().collect();
        let mut index = 0usize;
        let strict = self.strict_delimiter;
        self.strings.clear();
        self.objects.clear();
        if !strict {
            skip_leading(&chars, &mut index);
        }
        while index < chars.len() {
            let value: String = if chars[index] == '"' {
                extract_quoted_str(&chars, &mut index)?
            } else {
                let start = index;
                while index < chars.len()
                    && (strict || (chars[index] as u32) > b' ' as u32)
                    && chars[index] != delimiter
                {
                    index += 1;
                }
                chars[start..index].iter().collect()
            };
            self.add(value)?;
            if !strict {
                skip_leading(&chars, &mut index);
            }
            if index < chars.len() && chars[index] == delimiter {
                if index + 1 >= chars.len() {
                    self.add(String::new())?;
                }
                index += 1;
                if !strict {
                    skip_leading(&chars, &mut index);
                }
            }
        }
        Ok(())
    }

    /// `TStrings.AddPair`: `Name + NameValueSeparator + Value`.
    pub fn add_pair(&mut self, name: &str, value: &str) -> Result<(), ValueError> {
        self.add(format!("{name}{}{value}", self.name_value_separator))
            .map(|_| ())
    }

    /// `TStrings.Contains`: `IndexOf(S) >= 0` through the same comparison
    /// `IndexOf` uses.
    pub fn contains(&self, text: &str) -> bool {
        self.index_of(text) >= 0
    }

    /// `TStrings.ContainsName` (`SameText` of the name part).
    pub fn contains_name(&self, name: &str) -> bool {
        self.index_of_name(name) >= 0
    }

    /// `TStrings.LoadFromFile(FileName[, Encoding])`: the bytes through
    /// `xedit-io`'s `TStringList` reading (the byte order mark, else the
    /// ANSI code page) for the one-argument form, or through the given
    /// `TEncoding` for the two-argument one.
    pub fn load_from_file(&mut self, file_name: &str, encoding: Option<EncodingKind>) -> Result<(), ValueError> {
        let bytes = std::fs::read(file_name).map_err(|error| {
            ValueError::external_class(
                "EFOpenError",
                format!("Cannot open file \"{file_name}\". {}", os_error_text(&error)),
            )
        })?;
        let text = decode_file(&bytes, encoding);
        self.set_text(&text)?;
        Ok(())
    }

    /// `TStrings.SaveToFile(FileName[, Encoding])`: `Text` written in the
    /// given encoding (the ANSI code page and no byte order mark by default,
    /// the encoding's preamble otherwise).
    pub fn save_to_file(&mut self, file_name: &str, encoding: Option<EncodingKind>) -> Result<(), ValueError> {
        let bytes = encode_file(&self.text(), encoding);
        std::fs::write(file_name, bytes).map_err(|error| {
            ValueError::external_class(
                "EFCreateError",
                format!("Cannot create file \"{file_name}\". {}", os_error_text(&error)),
            )
        })
    }
}

/// The text of an OS error as Delphi raises it: the system text without
/// Rust's ` (os error N)` suffix and without the trailing period
/// (`SysErrorMessage`'s shape, which is what the oracle's `EFOpenError`/
/// `EFCreateError` lines carry: `The system cannot find the path
/// specified`).
pub(crate) fn os_error_text(error: &std::io::Error) -> String {
    let text = error.to_string();
    let text = match text.rfind(" (os error ") {
        Some(at) => text[..at].to_owned(),
        None => text,
    };
    match text.strip_suffix('.') {
        Some(stripped) => stripped.to_owned(),
        None => text,
    }
}

impl ScriptObject for StringListObject {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// The shared [`xedit_script::strings::StringList`] view the host
    /// functions of the element, file, resource and data format adapters
    /// fill (`Value::string_list_mut`).
    fn string_list(&self) -> Option<&dyn SharedStringList> {
        Some(self)
    }

    fn string_list_mut(&mut self) -> Option<&mut dyn SharedStringList> {
        Some(self)
    }
}

/// The shared trait the step-5 and step-6 adapter functions cast a
/// `TStrings` parameter to. The methods keep this surface's semantics (the
/// locale-aware `CompareStrings`, `Sorted`/`Duplicates` with the RTL's
/// `dupError`); the trait's index type is `i32` and its lines are bytes.
impl SharedStringList for StringListObject {
    fn lines(&self) -> &[Vec<u8>] {
        &self.strings
    }

    fn objects(&self) -> &[Value] {
        &self.objects
    }

    fn sorted(&self) -> bool {
        self.sorted
    }

    fn duplicates(&self) -> Duplicates {
        Duplicates::from_int(i64::from(self.duplicates))
    }

    fn clear(&mut self) {
        StringListObject::clear(self);
    }

    fn add(&mut self, text: Vec<u8>) -> i32 {
        // `dupError` is the RTL's `EStringListError`; the trait has no error
        // channel, so the port reports it as -1 (`xedit_script::strings`'s
        // convention).
        self.add_bytes(text).map_or(-1, |index| index as i32)
    }

    fn add_object(&mut self, text: Vec<u8>, object: Value) -> i32 {
        match self.add_bytes(text) {
            Ok(index) => {
                self.objects[index] = object;
                index as i32
            }
            Err(_) => -1,
        }
    }

    fn set_line(&mut self, index: i32, text: Vec<u8>) {
        if let Ok(index) = usize::try_from(index)
            && index < self.strings.len()
        {
            self.strings[index] = text;
        }
    }

    fn delete(&mut self, index: i32) {
        let _ = StringListObject::delete(self, i64::from(index));
    }

    fn insert(&mut self, index: i32, text: Vec<u8>, object: Value) {
        // `TStrings.Insert` refuses a sorted list; the trait has no error
        // channel, so the line is dropped there.
        if self.sorted {
            return;
        }
        let index = usize::try_from(index).unwrap_or(0).min(self.strings.len());
        self.strings.insert(index, text);
        self.objects.insert(index.min(self.objects.len()), object);
    }

    fn index_of(&self, text: &[u8]) -> i32 {
        self.index_of(&line_text(text)) as i32
    }

    // The members phase 6 step 8 extended the shared trait with for the
    // `Classes` unit's `TStrings`/`TStringList` surface.

    fn set_object(&mut self, index: i32, object: Value) {
        if let Ok(index) = usize::try_from(index)
            && index < self.objects.len()
        {
            self.objects[index] = object;
        }
    }

    fn compare_strings(&self, left: &[u8], right: &[u8]) -> std::cmp::Ordering {
        self.compare(&line_text(left), &line_text(right))
    }

    fn exchange(&mut self, first: i32, second: i32) {
        if let (Ok(first), Ok(second)) = (usize::try_from(first), usize::try_from(second))
            && first < self.strings.len()
            && second < self.strings.len()
        {
            self.strings.swap(first, second);
            if first < self.objects.len() && second < self.objects.len() {
                self.objects.swap(first, second);
            }
        }
    }

    fn move_line(&mut self, from: i32, to: i32) {
        if let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to))
            && from < self.strings.len()
            && to < self.strings.len()
        {
            let text = self.strings.remove(from);
            self.strings.insert(to, text);
            if from < self.objects.len() {
                let object = self.objects.remove(from);
                self.objects.insert(to.min(self.objects.len()), object);
            }
        }
    }

    fn set_sorted(&mut self, sorted: bool) {
        StringListObject::set_sorted(self, sorted);
    }

    fn set_duplicates(&mut self, duplicates: Duplicates) {
        self.duplicates = duplicates.as_int();
    }

    fn sort(&mut self) {
        StringListObject::sort(self);
    }
}

/// One stored line as text (the list keeps bytes, as the shared model does).
fn line_text(line: &[u8]) -> String {
    String::from_utf8_lossy(line).into_owned()
}

/// `Skip` of the delimited-text parser: the characters `<= ' '`.
fn skip_leading(chars: &[char], index: &mut usize) {
    while *index < chars.len() && (chars[*index] as u32) <= b' ' as u32 {
        *index += 1;
    }
}

/// `AnsiExtractQuotedStr(var P, Quote)`: `index` is at the opening quote;
/// the result is the text with doubled quotes collapsed and `index` moves
/// past the closing quote (or to the end when it is missing).
fn extract_quoted_str(chars: &[char], index: &mut usize) -> Result<String, ValueError> {
    const QUOTE: char = '"';
    *index += 1;
    let mut out = String::new();
    while *index < chars.len() {
        if chars[*index] == QUOTE {
            if *index + 1 < chars.len() && chars[*index + 1] == QUOTE {
                out.push(QUOTE);
                *index += 2;
                continue;
            }
            *index += 1;
            return Ok(out);
        }
        out.push(chars[*index]);
        *index += 1;
    }
    Ok(out)
}

/// A `TStringList` object, for the registrars that hold one.
pub fn new_list(classes: &ScriptClasses) -> Value {
    Value::Object(Rc::new(RefCell::new(StringListObject::new(
        classes.t_string_list.clone(),
    ))))
}

/// The `TStringList`/`TStrings` object behind a value, `None` for anything
/// else. Upstream hard-casts (`TStringList(Args.Obj)`); a cast of the wrong
/// object is undefined there, so the port refuses it instead.
pub fn list_of(value: &Value) -> Option<Rc<RefCell<dyn ScriptObject>>> {
    let object = value.as_object()?;
    if object.borrow().as_any().is::<StringListObject>() {
        Some(object.clone())
    } else {
        None
    }
}

/// [`list_of`] or an `EInvalidCast`.
pub fn list_of_or_error(value: &Value) -> Result<Rc<RefCell<dyn ScriptObject>>, ValueError> {
    list_of(value).ok_or_else(|| {
        ValueError::external_class(
            "EInvalidCast",
            "Invalid cast: the value is not a TStringList".to_owned(),
        )
    })
}

/// The receiver of a method call as a `TStringList`.
pub fn receiver(args: &Args) -> Result<Rc<RefCell<dyn ScriptObject>>, ValueError> {
    match &args.obj {
        Some(object) => list_of_or_error(object),
        None => Err(ValueError::external_class(
            "EInvalidCast",
            "Invalid cast: the value is not a TStringList".to_owned(),
        )),
    }
}

pub fn borrow_list(object: &Rc<RefCell<dyn ScriptObject>>) -> Ref<'_, StringListObject> {
    Ref::map(object.borrow(), |object| {
        object
            .as_any()
            .downcast_ref::<StringListObject>()
            .expect("a TStringList object")
    })
}

pub fn borrow_list_mut(object: &Rc<RefCell<dyn ScriptObject>>) -> RefMut<'_, StringListObject> {
    RefMut::map(object.borrow_mut(), |object| {
        object
            .as_any_mut()
            .downcast_mut::<StringListObject>()
            .expect("a TStringList object")
    })
}

//=== TEncoding ===============================================================

/// The four `TEncoding` objects a script can name (`TEncoding.Default`,
/// `TEncoding.ASCII`, `TEncoding.Unicode`, `TEncoding.UTF8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingKind {
    /// `TEncoding.Default`: the system ANSI code page, no byte order mark.
    Default,
    /// `TEncoding.ASCII`.
    Ascii,
    /// `TEncoding.Unicode`: UTF-16 little endian with a `FF FE` mark.
    Unicode,
    /// `TEncoding.UTF8` with a `EF BB BF` mark.
    Utf8,
}

impl EncodingKind {
    pub fn name(self) -> &'static str {
        match self {
            EncodingKind::Default => "Default",
            EncodingKind::Ascii => "ASCII",
            EncodingKind::Unicode => "Unicode",
            EncodingKind::Utf8 => "UTF8",
        }
    }

    /// `TEncoding.GetPreamble` (`SaveToStream` writes it in front of the
    /// text).
    pub fn preamble(self) -> &'static [u8] {
        match self {
            EncodingKind::Default | EncodingKind::Ascii => &[],
            EncodingKind::Unicode => &[0xFF, 0xFE],
            EncodingKind::Utf8 => &[0xEF, 0xBB, 0xBF],
        }
    }

    /// `TEncoding.GetBytes`.
    pub fn bytes(self, text: &str) -> Vec<u8> {
        match self {
            EncodingKind::Default => ansi_bytes(text),
            EncodingKind::Ascii => text
                .chars()
                .map(|ch| if (ch as u32) < 128 { ch as u8 } else { b'?' })
                .collect(),
            EncodingKind::Unicode => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            EncodingKind::Utf8 => text.as_bytes().to_vec(),
        }
    }

    /// `TEncoding.GetString`.
    pub fn string(self, bytes: &[u8]) -> String {
        match self {
            EncodingKind::Default => ansi_string(bytes),
            EncodingKind::Ascii => bytes
                .iter()
                .map(|byte| if *byte < 128 { *byte as char } else { '?' })
                .collect(),
            EncodingKind::Unicode => {
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                String::from_utf16_lossy(&units)
            }
            EncodingKind::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        }
    }
}

/// The script value of a `TEncoding`.
pub fn encoding_value(class: &Rc<ClassDef>, kind: EncodingKind) -> Value {
    Value::Object(Rc::new(RefCell::new(EncodingObject {
        class: class.clone(),
        kind,
    })))
}

/// One `TEncoding` object.
pub struct EncodingObject {
    class: Rc<ClassDef>,
    pub kind: EncodingKind,
}

impl ScriptObject for EncodingObject {
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

/// The `TEncoding` behind a value, if any.
pub fn encoding_of(value: &Value) -> Option<EncodingKind> {
    let object = value.as_object()?;
    let borrowed = object.borrow();
    borrowed
        .as_any()
        .downcast_ref::<EncodingObject>()
        .map(|object| object.kind)
}

/// `TStrings.LoadFromFile`: the one-argument form follows the byte order
/// mark (the port's `string_list_text`), the two-argument one the encoding;
/// a mark of the named encoding is skipped, as the RTL's decoders do.
fn decode_file(bytes: &[u8], encoding: Option<EncodingKind>) -> String {
    let Some(kind) = encoding else {
        return string_list_text(bytes);
    };
    let rest = bytes.strip_prefix(kind.preamble()).unwrap_or(bytes);
    kind.string(rest)
}

/// `TStrings.SaveToFile`: the encoding's preamble, then `Text`.
fn encode_file(text: &str, encoding: Option<EncodingKind>) -> Vec<u8> {
    let kind = encoding.unwrap_or(EncodingKind::Default);
    let mut bytes = kind.preamble().to_vec();
    bytes.extend(kind.bytes(text));
    bytes
}

//=== Registration: the slice of the JVCL Classes unit ========================

pub fn text_arg(value: &Value) -> Result<String, ValueError> {
    // `sl.Add(Null)` and `sl.Add(<element>)` are the variant cast errors
    // (`(Null) into type (OleStr)`, `(Unknown) into type (OleStr)`, probes
    // `x-289`, `x-288`); an object reads as `''`.
    crate::script_rtl::string_of(value).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// [`text_arg`] for the readers that cannot raise (a setter's branch, a
/// list of values): a value the conversion refuses reads as `''`.
pub fn text_arg_lossy(value: &Value) -> String {
    text_arg(value).unwrap_or_default()
}

pub fn int_arg(value: &Value) -> Result<i32, ValueError> {
    // An index that is no number is the variant cast error (`sl.Delete('x')`
    // is `Could not convert variant of type (UnicodeString) into type
    // (Integer)`, probe `x-311`).
    xedit_script::values::to_i32(value)
}

fn value_param() -> ParamType {
    ParamType::value(VarType::Empty)
}

/// The `TStrings`/`TStringList` base methods the Misc registrations and the
/// step tests exercise: the early slice of `JvInterpreter_Classes.pas`'s
/// `TStrings`/`TStringList` block. Step 8 extends it; the registrations
/// here are not ledger entries (the JVCL units are outside the phase 6
/// script ledger).
pub fn register_base(adapter: &mut Adapter, classes: &ScriptClasses) {
    // `TStringList_Create` (`JvInterpreter_Classes.pas:1282`), called on the
    // class value `TStringList` as upstream.
    {
        let class = classes.t_string_list.clone();
        adapter.add_get(
            classes.t_string_list.clone(),
            "Create",
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                *value = Value::Object(Rc::new(RefCell::new(StringListObject::new(class.clone()))));
                Ok(())
            }),
            0,
            Vec::new(),
            VarType::Empty,
        );
    }

    // `TStrings_Add`: the result is the index.
    adapter.add_get(
        classes.t_strings.clone(),
        "Add",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let text = text_arg(&args.values[0])?;
            let object = receiver(args)?;
            let index = borrow_list_mut(&object).add(text)?;
            *value = Value::Integer(index as i32);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );

    // `TStrings_Append`.
    adapter.add_get(
        classes.t_strings.clone(),
        "Append",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let text = text_arg(&args.values[0])?;
            let object = receiver(args)?;
            borrow_list_mut(&object).append(text)
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );

    // `TStrings_AddStrings`.
    adapter.add_get(
        classes.t_strings.clone(),
        "AddStrings",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let other = list_of_or_error(&args.values[0])?;
            let object = receiver(args)?;
            // `TStrings.AddStrings` adds every line with `AddObject`
            // (`System.Classes`), so the objects come along; `AddStrings(Self)`
            // reads the growing list, which a snapshot of its starting state
            // reproduces.
            let source = borrow_list(&other).snapshot();
            let mut list = borrow_list_mut(&object);
            for index in 0..source.count() {
                let text = source.line_bytes(index).unwrap_or_default().to_vec();
                let item = source.objects.get(index).cloned().unwrap_or(Value::Empty);
                let at = list.add_bytes(text)?;
                list.objects[at] = item;
            }
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );

    // `TStrings_Assign`.
    adapter.add_get(
        classes.t_strings.clone(),
        "Assign",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let other = list_of_or_error(&args.values[0])?;
            let object = receiver(args)?;
            if Rc::ptr_eq(&other, &object) {
                return Ok(());
            }
            let source = borrow_list(&other).snapshot();
            borrow_list_mut(&object).assign(&source);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );

    // `TStrings_Clear`.
    adapter.add_get(
        classes.t_strings.clone(),
        "Clear",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let object = receiver(args)?;
            borrow_list_mut(&object).clear();
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );

    // `TStrings_Read_Count`.
    adapter.add_get(
        classes.t_strings.clone(),
        "Count",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver(args)?;
            *value = Value::Integer(borrow_list(&object).count() as i32);
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );

    // `TStrings_Delete`.
    adapter.add_get(
        classes.t_strings.clone(),
        "Delete",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let index = int_arg(&args.values[0])?;
            let object = receiver(args)?;
            borrow_list_mut(&object).delete(i64::from(index))
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );

    // `TStrings_Insert`.
    adapter.add_get(
        classes.t_strings.clone(),
        "Insert",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let index = int_arg(&args.values[0])?;
            let text = text_arg(&args.values[1])?;
            let object = receiver(args)?;
            borrow_list_mut(&object).insert(i64::from(index), text)
        }),
        2,
        vec![value_param(), value_param()],
        VarType::Empty,
    );

    // `TStrings_Exchange`.
    adapter.add_get(
        classes.t_strings.clone(),
        "Exchange",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let first = int_arg(&args.values[0])?;
            let second = int_arg(&args.values[1])?;
            let object = receiver(args)?;
            borrow_list_mut(&object).exchange(i64::from(first), i64::from(second))
        }),
        2,
        vec![value_param(), value_param()],
        VarType::Empty,
    );

    // `TStrings_IndexOf`.
    adapter.add_get(
        classes.t_strings.clone(),
        "IndexOf",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let text = text_arg(&args.values[0])?;
            let object = receiver(args)?;
            *value = Value::Integer(borrow_list(&object).index_of(&text) as i32);
            Ok(())
        }),
        1,
        vec![value_param()],
        VarType::Empty,
    );

    // `TStrings_Read_Text` / `TStrings_Write_Text`.
    adapter.add_get(
        classes.t_strings.clone(),
        "Text",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver(args)?;
            *value = Value::Str(borrow_list(&object).text().into_bytes());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        classes.t_strings.clone(),
        "Text",
        Rc::new(|value: &Value, args: &mut Args| {
            let text = text_arg(value)?;
            let object = receiver(args)?;
            borrow_list_mut(&object).set_text(&text)?;
            Ok(())
        }),
        0,
        vec![value_param()],
    );

    // `TStrings_Read_Strings` / `TStrings_Write_Strings`: both the named
    // `Strings[I]` property and the default property `sl[I]` upstream
    // (`AddIGet`/`AddISet` *and* `AddIDGet`/`AddIDSet`, `:1268-1271`).
    for named in [true, false] {
        let class = classes.t_strings.clone();
        let read = Rc::new(|value: &mut Value, args: &mut Args| {
            let index = int_arg(&args.values[0])?;
            let object = receiver(args)?;
            let list = borrow_list(&object);
            if list.lenient_index && (index < 0 || index as usize >= list.strings.len()) {
                *value = Value::Str(Vec::new());
                return Ok(());
            }
            list.check_index(i64::from(index), list.strings.len())?;
            *value = Value::Str(list.string_at(i64::from(index)).into_bytes());
            Ok(())
        });
        let write = Rc::new(|value: &Value, args: &mut Args| {
            let index = int_arg(&args.values[0])?;
            let text = text_arg(value)?;
            let object = receiver(args)?;
            let mut list = borrow_list_mut(&object);
            list.check_index(i64::from(index), list.strings.len())?;
            list.set_string(i64::from(index), text);
            Ok(())
        });
        if named {
            adapter.add_iget(
                class.clone(),
                "Strings",
                read.clone(),
                1,
                vec![value_param()],
                VarType::Empty,
            );
            adapter.add_iset(class.clone(), "Strings", write.clone(), 1, vec![value_param()]);
            // No `Strings` reader or writer without the index: upstream
            // registers none, and the bare read the assignment
            // `sl.Strings[0] := s` opens with (`InternalSetValue`) found the
            // port's call-form reader and refused it (`Not enough
            // parameters`, found by issue #25; nothing used the call form).
        } else {
            adapter.add_id_get(class.clone(), read, 1, vec![value_param()], VarType::Empty);
            adapter.add_id_set(class, write, 1, vec![value_param()]);
        }
    }

    // `TStringList_Sort`.
    adapter.add_get(
        classes.t_string_list.clone(),
        "Sort",
        Rc::new(|_value: &mut Value, args: &mut Args| {
            let object = receiver(args)?;
            borrow_list_mut(&object).sort();
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );

    // `TStringList_Find(S, var Index)`.
    adapter.add_get(
        classes.t_string_list.clone(),
        "Find",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let text = text_arg(&args.values[0])?;
            let object = receiver(args)?;
            let (found, index) = borrow_list(&object).find(&text);
            if args.values.len() > 1 {
                args.values[1] = Value::Integer(index as i32);
            }
            *value = Value::Bool(found);
            Ok(())
        }),
        2,
        vec![value_param(), ParamType::by_ref(VarType::Empty)],
        VarType::Empty,
    );

    // `TStringList_Read_Sorted` / `TStringList_Write_Sorted`.
    adapter.add_get(
        classes.t_string_list.clone(),
        "Sorted",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver(args)?;
            *value = Value::Bool(borrow_list(&object).sorted());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        classes.t_string_list.clone(),
        "Sorted",
        Rc::new(|value: &Value, args: &mut Args| {
            let sorted = xedit_script::values::to_bool(value)?;
            let object = receiver(args)?;
            borrow_list_mut(&object).set_sorted(sorted);
            Ok(())
        }),
        0,
        vec![value_param()],
    );

    // `TStringList_Read_Duplicates` / `TStringList_Write_Duplicates`.
    adapter.add_get(
        classes.t_string_list.clone(),
        "Duplicates",
        Rc::new(|value: &mut Value, args: &mut Args| {
            let object = receiver(args)?;
            *value = Value::Integer(borrow_list(&object).duplicates());
            Ok(())
        }),
        0,
        Vec::new(),
        VarType::Empty,
    );
    adapter.add_set(
        classes.t_string_list.clone(),
        "Duplicates",
        Rc::new(|value: &Value, args: &mut Args| {
            let duplicates = int_arg(value)?;
            let object = receiver(args)?;
            borrow_list_mut(&object).set_duplicates(duplicates);
            Ok(())
        }),
        0,
        vec![value_param()],
    );

    // `TEncoding.Default` and friends (`xejviScriptAdapterMisc.pas:884-887`).
    for (identifier, kind) in [
        ("Default", EncodingKind::Default),
        ("ASCII", EncodingKind::Ascii),
        ("Unicode", EncodingKind::Unicode),
        ("UTF8", EncodingKind::Utf8),
    ] {
        let class = classes.t_encoding.clone();
        adapter.add_get(
            classes.t_encoding.clone(),
            identifier,
            Rc::new(move |value: &mut Value, _args: &mut Args| {
                *value = encoding_value(&class, kind);
                Ok(())
            }),
            0,
            Vec::new(),
            VarType::Empty,
        );
    }
}
