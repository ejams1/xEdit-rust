// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/jvcl/jvcl/run/JvInterpreter_Classes.pas
// (`TList`) as far as the adapter functions that fill a `TList` need it.

//! The `TList` a script passes to a host function.
//!
//! `wbGetSiblingRecords` and `wbFindREFRsByBase` (`xejviScriptAdapterResource.pas`)
//! append object pointers to a `TList` the script created and later reads
//! with `lst[i]`. The port keeps the elements as their script values
//! (the `IwbElement` object the element convention of phase 6 step 4 hands
//! out), which is what a script's own `ObjectToElement(lst[i])` reads back;
//! upstream stores `Pointer(IwbMainRecord)`.
//!
//! `TList` itself is part of the JVCL `Classes` unit (phase 6 step 8); like
//! `TStringList`, the class is registered here as far as these functions need
//! it and a later step extends the same class.

use std::cell::RefCell;
use std::rc::Rc;

use crate::values::{ClassDef, ScriptObject, Value};

/// The `TList` interface the adapter functions use.
///
/// The `Classes` unit of phase 6 step 8 registers the whole `TList` member
/// set on top of this trait, so the operations its methods need beyond the
/// host functions' three (`set`, the removes and moves, the capacity) are
/// here too; every implementation is the same list, so the port has one
/// object for a `TList` however it was created.
pub trait List: ScriptObject {
    /// The entries, in order.
    fn items(&self) -> &[Value];
    /// `TList.Add`: appends and returns the index.
    fn add(&mut self, value: Value) -> i32;
    /// `TList[Index]`.
    fn get(&self, index: i32) -> Option<&Value>;
    /// `TList.Clear`.
    fn clear(&mut self);
    /// `TList.Count`.
    fn count(&self) -> i32 {
        self.items().len() as i32
    }
    /// `TList.Items[Index] := Value` (`AddISet`/`AddIDSet`).
    fn set(&mut self, index: i32, value: Value);
    /// `TList.Delete`.
    fn delete_at(&mut self, index: i32);
    /// `TList.Insert`.
    fn insert(&mut self, index: i32, value: Value);
    /// `TList.Exchange`.
    fn exchange(&mut self, first: i32, second: i32);
    /// `TList.Move`.
    fn move_item(&mut self, from: i32, to: i32);
    /// `TList.Remove`: the index the item had, -1 when it is not there.
    fn remove(&mut self, value: &Value) -> i32;
    /// `TList.Pack`: drops the nil entries.
    fn pack(&mut self);
    /// `TList.Count := N`: grows with nil entries or truncates.
    fn resize(&mut self, count: usize);
    /// `TList.Capacity`.
    fn capacity(&self) -> usize {
        self.items().len()
    }
    /// `TList.Capacity := N`.
    fn reserve(&mut self, _capacity: usize) {}
}

/// The `TList` the host owns.
pub struct ScriptList {
    items: Vec<Value>,
    class: Rc<ClassDef>,
}

impl ScriptList {
    pub fn new(class: Rc<ClassDef>) -> ScriptList {
        ScriptList {
            items: Vec::new(),
            class,
        }
    }
}

impl ScriptObject for ScriptList {
    fn class(&self) -> Rc<ClassDef> {
        self.class.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn list(&self) -> Option<&dyn List> {
        Some(self)
    }

    fn list_mut(&mut self) -> Option<&mut dyn List> {
        Some(self)
    }
}

impl List for ScriptList {
    fn items(&self) -> &[Value] {
        &self.items
    }

    fn add(&mut self, value: Value) -> i32 {
        self.items.push(value);
        (self.items.len() - 1) as i32
    }

    fn get(&self, index: i32) -> Option<&Value> {
        usize::try_from(index).ok().and_then(|index| self.items.get(index))
    }

    fn clear(&mut self) {
        self.items.clear();
    }

    fn set(&mut self, index: i32, value: Value) {
        if let Some(slot) = usize::try_from(index).ok().and_then(|index| self.items.get_mut(index)) {
            *slot = value;
        }
    }

    fn delete_at(&mut self, index: i32) {
        if let Some(index) = usize::try_from(index).ok().filter(|index| *index < self.items.len()) {
            self.items.remove(index);
        }
    }

    fn insert(&mut self, index: i32, value: Value) {
        let index = usize::try_from(index).unwrap_or(0).min(self.items.len());
        self.items.insert(index, value);
    }

    fn exchange(&mut self, first: i32, second: i32) {
        let first = usize::try_from(first).ok();
        let second = usize::try_from(second).ok();
        if let (Some(first), Some(second)) = (first, second)
            && first < self.items.len()
            && second < self.items.len()
        {
            self.items.swap(first, second);
        }
    }

    fn move_item(&mut self, from: i32, to: i32) {
        let from = usize::try_from(from).ok();
        let to = usize::try_from(to).ok();
        if let (Some(from), Some(to)) = (from, to)
            && from < self.items.len()
            && to < self.items.len()
        {
            let value = self.items.remove(from);
            self.items.insert(to, value);
        }
    }

    fn remove(&mut self, value: &Value) -> i32 {
        match self.items.iter().position(|entry| entry == value) {
            Some(index) => {
                self.items.remove(index);
                index as i32
            }
            None => -1,
        }
    }

    fn pack(&mut self) {
        self.items
            .retain(|item| !matches!(item, Value::Pointer(0) | Value::Null));
    }

    fn resize(&mut self, count: usize) {
        self.items.resize(count, Value::Pointer(0));
    }

    fn capacity(&self) -> usize {
        self.items.capacity()
    }

    fn reserve(&mut self, capacity: usize) {
        self.items.reserve(capacity);
    }
}

/// Builds the list value of a `TList`.
pub fn list_value(class: Rc<ClassDef>) -> Value {
    Value::Object(Rc::new(RefCell::new(ScriptList::new(class))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_appends_and_reads_back() {
        let class = Rc::new(ClassDef {
            unit: "Classes".to_owned(),
            name: "TList".to_owned(),
            parent: None,
        });
        let mut list = ScriptList::new(class);
        assert_eq!(list.add(Value::Integer(7)), 0);
        assert_eq!(list.count(), 1);
        assert_eq!(list.get(0), Some(&Value::Integer(7)));
        list.clear();
        assert_eq!(list.count(), 0);
    }
}
