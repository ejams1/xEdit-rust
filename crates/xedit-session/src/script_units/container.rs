// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterContainer.pas

//! `xejviScriptAdapterContainer.pas`: the `IwbContainer` functions, bound to
//! the container of an element (`xedit_core::interface::Container`).
//!
//! `Add` (3 args) is upstream's add-by-name over the container and the
//! session command `elements.add`; here it takes the element, the name and
//! the silent flag of the script value.

use std::cell::RefCell;
use std::rc::Rc;

use xedit_core::interface::FileState;
use xedit_script::adapter::{Adapter, Args};
use xedit_script::values::{ParamType, Value, ValueError, VarType, to_bool};

use super::{element_at, integer_at, signature_at, string_at};
use crate::script::ScriptRunState;

pub(crate) fn register(adapter: &mut Adapter, state: &Rc<RefCell<ScriptRunState>>) {
    let value = || vec![ParamType::value(VarType::Empty)];
    let value_string = || vec![ParamType::value(VarType::Empty), ParamType::value(VarType::Str)];
    let value_integer = || vec![ParamType::value(VarType::Empty), ParamType::value(VarType::Integer)];
    let state_value = state.clone();

    // `Value := Container.Add(Args.Values[1], Args.Values[2])`: the member is
    // added (or found) and the result is the element, `Unassigned` when the
    // container could not add it.
    {
        let state = state_value.clone();
        adapter.add_function(
            super::C_UNIT,
            "Add",
            Rc::new(move |result: &mut Value, args: &mut Args| {
                let Some(element) = element_at(args, 0) else {
                    return Ok(());
                };
                let Some(container) = element.as_container() else {
                    return Ok(());
                };
                let name = string_at(args, 1);
                let silent = to_bool(args.values.get(2).unwrap_or(&Value::Empty)).unwrap_or(false);
                if let Some(added) = super::refused(container.add(&name, silent))? {
                    *result = state.borrow().element_value(&added);
                }
                Ok(())
            }),
            3,
            {
                let mut params = value();
                params.push(ParamType::value(VarType::Str));
                params.push(ParamType::value(VarType::Bool));
                params
            },
            VarType::Empty,
        );
    }
    // `Container.AddElement(Element)`: the element takes a reference to the
    // added one; the port keeps the `Add` of its own name/add path.
    adapter.add_function(
        super::C_UNIT,
        "AddElement",
        Rc::new(|_result: &mut Value, args: &mut Args| {
            let (Some(element), Some(source)) = (element_at(args, 0), element_at(args, 1)) else {
                return Ok(());
            };
            if let Some(container) = element.as_container() {
                let name = source.get_name();
                super::refused(container.add(&name, true))?;
            }
            Ok(())
        }),
        2,
        vec![ParamType::value(VarType::Empty), ParamType::value(VarType::Empty)],
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "AdditionalElementCount",
        Rc::new(|result: &mut Value, args: &mut Args| {
            *result = Value::Integer(0);
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
            {
                *result = Value::Integer(container.get_additional_element_count());
            }
            Ok(())
        }),
        1,
        value(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "ContainerStates",
        Rc::new(|result: &mut Value, args: &mut Args| {
            // `Value := Word(Container.ContainerStates)`: the set as a bit
            // mask (`csInit` 1, `csInitOnce` 2, `csInitDone` 4,
            // `csInitializing` 8, `csReseting` 16, `csRefsBuild` 32,
            // `csAsCreatedEmpty` 64, `csSortedBySortOrder` 128,
            // `csCollapsed` 256, `csExpanded` 512, `csConstructionCompleted`
            // 1024). A container the port has built answers the states a
            // `DoInit`ed container of a finished construction holds
            // (`el-states` compares the file, a record and a group), and the
            // references a file built or loaded add `csRefsBuild`. An
            // element that is no container leaves the result unassigned, as
            // the adapter's failed `Supports` does.
            let Some(element) = element_at(args, 0) else {
                return Ok(());
            };
            if element.as_container().is_none() {
                return Ok(());
            }
            // `DoInit` includes `csInit`, `csInitOnce` and `csInitDone`
            // (1031 with `csConstructionCompleted`); `DoReset` excludes the
            // first and the last again when the container's last reference
            // goes and `CanElementReset` allows it, so a container that was
            // read once and let go answers 1026 and one never read 1024.
            // The file and a group stay held. A main record and a subrecord
            // may reset while no element of theirs is referenced from
            // outside (`cntElementRefs < 1`): the record a script holds a
            // subrecord of, or the `DATA` it holds the value of, is 1031,
            // the same after the reference is gone 1026 (`el-states`). A
            // value (the record header) may reset only while nothing refers
            // to the value itself (`eExternalRefs < 1`), and the script's
            // reference is there while it asks: once read it is 1031.
            // UPSTREAM-NOTE: the port does not reset on a release, so a
            // value read, let go and asked about again answers 1031 where
            // the oracle may have reset it to 1026.
            let full = 1 | 2 | 4 | 1024;
            let element_impl = element.as_element_impl();
            let mut states: i32 = match element.get_element_type() {
                xedit_core::interface::ElementType::etFile | xedit_core::interface::ElementType::etGroupRecord => full,
                _ => {
                    let held_by_elements = element_impl
                        .and_then(|element| element.container_base())
                        .is_some_and(|container| container.is_element_referenced());
                    let (init_once, is_value) = match element_impl.and_then(|element| element.main_record_impl()) {
                        Some(record) => (record.is_initialized(), false),
                        None => (
                            element_impl
                                .and_then(|element| element.container_base())
                                .is_some_and(|container| container.is_init_once()),
                            element_impl.is_some_and(|element| element.value_impl().is_some()),
                        ),
                    };
                    if !init_once {
                        1024
                    } else if is_value || held_by_elements {
                        full
                    } else {
                        2 | 1024
                    }
                }
            };
            if let Some(file) = element.get_file()
                && file.get_file_states().contains(FileState::fsRefsBuild)
            {
                states |= 32;
            }
            *result = Value::Integer(states);
            Ok(())
        }),
        1,
        value(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "ElementByIndex",
        Rc::new({
            let state = state_value.clone();
            move |result: &mut Value, args: &mut Args| {
                let Some(element) = element_at(args, 0) else {
                    return Ok(());
                };
                let Some(container) = element.as_container() else {
                    return Ok(());
                };
                let index = integer_at(args, 1)? as i32;
                if let Some(child) = container.get_element(index) {
                    *result = state.borrow().element_value(&child);
                }
                Ok(())
            }
        }),
        2,
        value_integer(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "ElementByName",
        Rc::new({
            let state = state_value.clone();
            move |result: &mut Value, args: &mut Args| {
                let Some(element) = element_at(args, 0) else {
                    return Ok(());
                };
                let Some(container) = element.as_container() else {
                    return Ok(());
                };
                let name = string_at(args, 1);
                if let Some(child) = container.get_element_by_name(&name) {
                    *result = state.borrow().element_value(&child);
                }
                Ok(())
            }
        }),
        2,
        value_string(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "ElementByPath",
        Rc::new({
            let state = state_value.clone();
            move |result: &mut Value, args: &mut Args| {
                let Some(element) = element_at(args, 0) else {
                    return Ok(());
                };
                let Some(container) = element.as_container() else {
                    return Ok(());
                };
                let path = string_at(args, 1);
                if let Some(child) = container.get_element_by_path(&path) {
                    *result = state.borrow().element_value(&child);
                }
                Ok(())
            }
        }),
        2,
        value_string(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "ElementBySignature",
        Rc::new({
            let state = state_value.clone();
            move |result: &mut Value, args: &mut Args| {
                let Some(element) = element_at(args, 0) else {
                    return Ok(());
                };
                let Some(container) = element.as_container() else {
                    return Ok(());
                };
                let signature = signature_at(args, 1);
                if let Some(child) = container.get_element_by_signature(signature) {
                    *result = state.borrow().element_value(&child);
                }
                Ok(())
            }
        }),
        2,
        value_string(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "ElementCount",
        Rc::new(|result: &mut Value, args: &mut Args| {
            *result = Value::Integer(0);
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
            {
                *result = Value::Integer(container.get_element_count());
            }
            Ok(())
        }),
        1,
        value(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "ElementExists",
        Rc::new(|result: &mut Value, args: &mut Args| {
            *result = Value::Bool(false);
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
            {
                // `TwbContainer.GetElementExists` is an `ElementByPath`
                // lookup: a name, a path or a signature.
                *result = Value::Bool(container.get_element_by_path(&string_at(args, 1)).is_some());
            }
            Ok(())
        }),
        2,
        value_string(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "GetElementEditValues",
        Rc::new(|result: &mut Value, args: &mut Args| {
            *result = Value::Str(Vec::new());
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
            {
                *result = Value::Str(container.get_element_edit_value(&string_at(args, 1)).into_bytes());
            }
            Ok(())
        }),
        2,
        value_string(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "GetElementNativeValues",
        Rc::new(|result: &mut Value, args: &mut Args| {
            // A path that resolves to nothing leaves `Value` unassigned
            // (probe `v-slot-56`).
            *result = Value::Empty;
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
                && container.get_element_by_path(&string_at(args, 1)).is_some()
            {
                *result = super::native_to_value(container.get_element_native_value(&string_at(args, 1)));
            }
            Ok(())
        }),
        2,
        value_string(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "GetElementValues",
        Rc::new(|result: &mut Value, args: &mut Args| {
            *result = Value::Str(Vec::new());
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
                && let Some(child) = container.get_element_by_path(&string_at(args, 1))
            {
                *result = Value::Str(child.get_value().into_bytes());
            }
            Ok(())
        }),
        2,
        value_string(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "IndexOf",
        Rc::new(|result: &mut Value, args: &mut Args| {
            *result = Value::Integer(-1);
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
                && let Some(child) = element_at(args, 1)
            {
                *result = Value::Integer(container.get_index_of(&child));
            }
            Ok(())
        }),
        2,
        vec![ParamType::value(VarType::Empty), ParamType::value(VarType::Empty)],
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "InsertElement",
        Rc::new(|_result: &mut Value, args: &mut Args| {
            let (Some(element), Some(source)) = (element_at(args, 0), element_at(args, 2)) else {
                return Ok(());
            };
            if element.as_container().is_none() {
                return Ok(());
            }
            let index = integer_at(args, 1)? as i32;
            // `TwbContainer.InsertElement`: the element, which has no
            // container (one `RemoveElement` gave), goes in at the position.
            xedit_core::implementation::moving::insert_element(&element, index, &source)
                .map_err(ValueError::External)?;
            Ok(())
        }),
        3,
        vec![
            ParamType::value(VarType::Empty),
            ParamType::value(VarType::Integer),
            ParamType::value(VarType::Empty),
        ],
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "IsSorted",
        Rc::new(|result: &mut Value, _args: &mut Args| {
            // `IwbSortableContainer.Sorted`: the port's sortable containers
            // (`TwbMainRecord`'s child group order) keep the flag on the
            // implementation; no `Container` method reads it yet.
            *result = Value::Bool(false);
            Ok(())
        }),
        1,
        value(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "LastElement",
        Rc::new({
            let state = state_value.clone();
            move |result: &mut Value, args: &mut Args| {
                if let Some(element) = element_at(args, 0)
                    && let Some(container) = element.as_container()
                    && let Some(last) = container.get_last_element()
                {
                    *result = state.borrow().element_value(&last);
                }
                Ok(())
            }
        }),
        1,
        value(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "RemoveByIndex",
        Rc::new(|result: &mut Value, args: &mut Args| {
            *result = Value::Bool(false);
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
            {
                let index = integer_at(args, 1)? as i32;
                let mark = to_bool(args.values.get(2).unwrap_or(&Value::Empty)).unwrap_or(true);
                *result = Value::Bool(container.remove_element_at(index, mark).is_some());
            }
            Ok(())
        }),
        3,
        vec![
            ParamType::value(VarType::Empty),
            ParamType::value(VarType::Integer),
            ParamType::value(VarType::Bool),
        ],
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "RemoveElement",
        {
            let state = state.clone();
            Rc::new(move |result: &mut Value, args: &mut Args| {
                let Some(element) = element_at(args, 0) else {
                    return Ok(());
                };
                let Some(container) = element.as_container() else {
                    return Ok(());
                };
                // `case VarType(Args.Values[1]) of varInteger, ...: remove by
                // index; varString: remove by name; else: remove the
                // element`; the result is the element removed
                // (`Container.RemoveElement(...)`), which has no container
                // any more and can go into one with `InsertElement`.
                let removed = match args.values.get(1).map(Value::var_type) {
                    Some(VarType::Integer | VarType::Smallint | VarType::Byte | VarType::Int64) => {
                        let index = integer_at(args, 1)? as i32;
                        container.remove_element_at(index, true)
                    }
                    Some(VarType::Str) => {
                        let name = string_at(args, 1);
                        container.remove_element_by_name(&name)
                    }
                    _ => match element_at(args, 1) {
                        Some(child) => {
                            let index = container.get_index_of(&child);
                            if index >= 0 {
                                container.remove_element_at(index, true)
                            } else {
                                None
                            }
                        }
                        None => None,
                    },
                };
                if let Some(removed) = removed {
                    *result = state.borrow().element_value(&removed);
                }
                Ok(())
            })
        },
        2,
        vec![ParamType::value(VarType::Empty), ParamType::value(VarType::Empty)],
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "ReverseElements",
        Rc::new(|_result: &mut Value, args: &mut Args| {
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
            {
                container.reverse_elements();
            }
            Ok(())
        }),
        1,
        value(),
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "SetElementEditValues",
        Rc::new(|_result: &mut Value, args: &mut Args| {
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
            {
                let path = string_at(args, 1);
                let value = string_at(args, 2);
                // Upstream `Container.ElementEditValues[path] := value`
                // raises `Exception.Create(GetName + ' can not be edited')`
                // (`wbImplementation.pas:16444-16468`) and aborts the run;
                // the discarded error made a refused edit a silent no-op.
                if let Err(message) = container.set_element_edit_value(&path, &value) {
                    return Err(ValueError::external_class("Exception", message));
                }
            }
            Ok(())
        }),
        3,
        vec![
            ParamType::value(VarType::Empty),
            ParamType::value(VarType::Str),
            ParamType::value(VarType::Str),
        ],
        VarType::Empty,
    );
    adapter.add_function(
        super::C_UNIT,
        "SetElementNativeValues",
        Rc::new(|_result: &mut Value, args: &mut Args| {
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.as_container()
            {
                let path = string_at(args, 1);
                let value = super::value_to_native(args.values.get(2).unwrap_or(&Value::Empty));
                if let Ok(value) = value
                    && let Err(message) = container.set_element_native_value(&path, value)
                {
                    return Err(ValueError::external_class("Exception", message));
                }
            }
            Ok(())
        }),
        3,
        vec![
            ParamType::value(VarType::Empty),
            ParamType::value(VarType::Str),
            ParamType::value(VarType::Empty),
        ],
        VarType::Empty,
    );
}
