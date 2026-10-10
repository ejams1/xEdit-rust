// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_Classes.pas
// (`TStrings`/`TStringList`) as far as the host functions of the adapter
// units take a `TStrings` parameter.

//! The `TStrings` a script passes to a host function.
//!
//! Several adapter functions take or fill a `TStrings`
//! (`IwbFile.GetMasters`, `IwbElement.ReportRequiredMasters`, the resource
//! lists of `xejviScriptAdapterResource.pas`, the NIF scanners), and
//! `TStringList` itself is the `Classes` unit of the JVCL
//! (`JvInterpreter_Classes.pas:624`, the `AddClass`/`AddGet` set of step 8).
//! So that the sessions of steps 5 to 9 share one value, the port models the
//! list here and exposes it two ways:
//!
//! * [`StringList`]: the lines and objects of the list, which the adapter
//!   functions read and write through [`crate::values::Value::string_list`]
//!   and [`crate::values::Value::string_list_mut`] whatever class registered
//!   the value.
//! * [`ScriptStringList`]: the one concrete list the port ships
//!   (`TStringList` itself), with the `Sorted`/`Duplicates` add semantics of
//!   Delphi's `TStrings.Add`.
//!
//! A later step that registers the `Classes` unit extends this type (or
//! implements [`StringList`] on its own) instead of registering a second
//! `TStringList` class, which the interpreter would resolve only through the
//! first of the two.

use std::cell::RefCell;
use std::rc::Rc;

use crate::values::{ClassDef, ScriptObject, Value};

/// `TDuplicates` (`System.Classes`).
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duplicates {
    dupIgnore,
    dupAccept,
    dupError,
}

impl Duplicates {
    pub fn from_int(value: i64) -> Duplicates {
        match value {
            1 => Duplicates::dupAccept,
            2 => Duplicates::dupError,
            _ => Duplicates::dupIgnore,
        }
    }

    pub fn as_int(self) -> i32 {
        match self {
            Duplicates::dupIgnore => 0,
            Duplicates::dupAccept => 1,
            Duplicates::dupError => 2,
        }
    }
}

/// The `TStrings` interface the adapter functions use.
///
/// Upstream they cast the object to `TStrings` and call its methods; the
/// port keeps the same shape: a value's [`ScriptObject::string_list`] is the
/// list, and a value of another class is not one (upstream's unchecked cast
/// has no defined behaviour, the port leaves the function a no-op).
pub trait StringList: ScriptObject {
    /// The lines, in order.
    fn lines(&self) -> &[Vec<u8>];
    /// The objects of the lines (`TStrings.Objects`), one per line.
    fn objects(&self) -> &[Value];
    /// `TStrings.Sorted`.
    fn sorted(&self) -> bool;
    /// `TStrings.Duplicates`.
    fn duplicates(&self) -> Duplicates;
    /// Replaces every line without its objects.
    fn clear(&mut self);
    /// `TStrings.Add`, with the `Sorted`/`Duplicates` behaviour of the list;
    /// the index the line took.
    fn add(&mut self, text: Vec<u8>) -> i32;
    /// `TStrings.AddObject`: the line with its object; the index.
    fn add_object(&mut self, text: Vec<u8>, object: Value) -> i32;
    /// `TStrings.Strings[i] := value`: the line at an index that exists.
    fn set_line(&mut self, index: i32, text: Vec<u8>);
    /// `TStrings.Delete`.
    fn delete(&mut self, index: i32);
    /// `TStrings.Insert`: the line and object at an index, unsorted.
    fn insert(&mut self, index: i32, text: Vec<u8>, object: Value);
    /// `TStrings.IndexOf` (`Find` on a sorted list).
    fn index_of(&self, text: &[u8]) -> i32;
}

/// `TStringList`: a `StringList` the host owns, with the class an `AddClass`
/// registered.
pub struct ScriptStringList {
    pub lines: Vec<Vec<u8>>,
    pub objects: Vec<Value>,
    pub is_sorted: bool,
    pub dups: Duplicates,
    /// `TStrings.CaseSensitive` (Delphi 11+; upstream xEdit leaves it at its
    /// default).
    pub case_sensitive: bool,
    class: Rc<ClassDef>,
}

impl ScriptStringList {
    pub fn new(class: Rc<ClassDef>) -> ScriptStringList {
        ScriptStringList {
            lines: Vec::new(),
            objects: Vec::new(),
            is_sorted: false,
            dups: Duplicates::dupIgnore,
            case_sensitive: false,
            class,
        }
    }

    /// The index of a line for the list's collation: `TStringList.Find` on a
    /// sorted list, a linear scan otherwise. `Ok(index)` found, `Err(index)`
    /// is where an insert belongs.
    fn find(&self, text: &[u8]) -> Result<usize, usize> {
        self.lines.binary_search_by(|line| self.compare(line, text))
    }

    /// `TStringList.CompareStrings`: `AnsiCompareText` of the two lines.
    /// The port folds ASCII case and compares the bytes, where upstream uses
    /// the user locale's word sort (`TStringList` sets no `UseLocale`, so
    /// punctuation orders before digits there); the difference shows in a
    /// sorted list only and is owed to `cargo xtask parity script`.
    fn compare(&self, left: &[u8], right: &[u8]) -> std::cmp::Ordering {
        if self.case_sensitive {
            return left.cmp(right);
        }
        left.iter()
            .map(u8::to_ascii_lowercase)
            .cmp(right.iter().map(u8::to_ascii_lowercase))
    }
}

impl ScriptObject for ScriptStringList {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn string_list(&self) -> Option<&dyn StringList> {
        Some(self)
    }

    fn string_list_mut(&mut self) -> Option<&mut dyn StringList> {
        Some(self)
    }
}

impl StringList for ScriptStringList {
    fn lines(&self) -> &[Vec<u8>] {
        &self.lines
    }

    fn objects(&self) -> &[Value] {
        &self.objects
    }

    fn sorted(&self) -> bool {
        self.is_sorted
    }

    fn duplicates(&self) -> Duplicates {
        self.dups
    }

    fn clear(&mut self) {
        self.lines.clear();
        self.objects.clear();
    }

    fn add(&mut self, text: Vec<u8>) -> i32 {
        self.add_object(text, Value::Empty)
    }

    fn add_object(&mut self, text: Vec<u8>, object: Value) -> i32 {
        if self.is_sorted {
            // `TStringList.Add` on a sorted list: `Find`, then `dupIgnore`
            // returns the existing index, `dupError` raises (which the port
            // reports as an invalid argument), `dupAccept` inserts.
            match self.find(&text) {
                Ok(index) => {
                    if self.dups == Duplicates::dupIgnore {
                        return index as i32;
                    }
                    let at = if self.dups == Duplicates::dupError {
                        // `EStringListError`; the function's caller reports
                        // the interpreter error.
                        return -1;
                    } else {
                        index + 1
                    };
                    self.lines.insert(at, text);
                    self.objects.insert(at, object);
                    return at as i32;
                }
                Err(index) => {
                    self.lines.insert(index, text);
                    self.objects.insert(index, object);
                    return index as i32;
                }
            }
        }
        self.lines.push(text);
        self.objects.push(object);
        (self.lines.len() - 1) as i32
    }

    fn set_line(&mut self, index: i32, text: Vec<u8>) {
        if let Some(line) = usize::try_from(index).ok().and_then(|index| self.lines.get_mut(index)) {
            *line = text;
        }
    }

    fn delete(&mut self, index: i32) {
        if let Some(index) = usize::try_from(index).ok().filter(|index| *index < self.lines.len()) {
            self.lines.remove(index);
            if index < self.objects.len() {
                self.objects.remove(index);
            }
        }
    }

    fn insert(&mut self, index: i32, text: Vec<u8>, object: Value) {
        let index = usize::try_from(index).unwrap_or(0).min(self.lines.len());
        self.lines.insert(index, text);
        self.objects.insert(index.min(self.objects.len()), object);
    }

    fn index_of(&self, text: &[u8]) -> i32 {
        if self.is_sorted {
            return match self.find(text) {
                Ok(index) => index as i32,
                Err(_) => -1,
            };
        }
        self.lines
            .iter()
            .position(|line| self.compare(line, text) == std::cmp::Ordering::Equal)
            .map(|index| index as i32)
            .unwrap_or(-1)
    }
}

/// Builds the list value of a `TStringList`.
pub fn string_list_value(class: Rc<ClassDef>) -> Value {
    Value::Object(Rc::new(RefCell::new(ScriptStringList::new(class))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> ScriptStringList {
        ScriptStringList::new(Rc::new(ClassDef {
            unit: "Classes".to_owned(),
            name: "TStringList".to_owned(),
            parent: None,
        }))
    }

    #[test]
    fn an_unsorted_list_appends() {
        let mut list = list();
        assert_eq!(list.add(b"b".to_vec()), 0);
        assert_eq!(list.add(b"a".to_vec()), 1);
        assert_eq!(list.lines(), &[b"b".to_vec(), b"a".to_vec()]);
        assert_eq!(list.index_of(b"a"), 1);
        assert_eq!(list.index_of(b"c"), -1);
    }

    #[test]
    fn a_sorted_list_keeps_case_insensitive_order_and_ignores_duplicates() {
        let mut list = list();
        list.is_sorted = true;
        assert_eq!(list.add(b"b".to_vec()), 0);
        assert_eq!(list.add(b"A".to_vec()), 0);
        // `Find` matches case-insensitively, so "a" is the "A" that is
        // there: `dupIgnore` returns its index and adds nothing.
        assert_eq!(list.add(b"a".to_vec()), 0);
        assert_eq!(list.lines(), &[b"A".to_vec(), b"b".to_vec()]);
    }
}
