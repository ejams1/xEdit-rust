// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterElement.pas

//! `xejviScriptAdapterElement.pas`: the `IwbElement` functions over the
//! element tree of `xedit_core::interface::Element`.
//!
//! `AssignTemplateByIndex`/`AssignTemplateByName`/`AssignTemplateCount` and
//! `TemplateAssign` read the templates of the definitions
//! (`xedit_core::implementation::template`), `MoveUp`/`MoveDown`/
//! `CanMoveUp`/`CanMoveDown` move entries of unsorted arrays
//! (`xedit_core::implementation::moving`), as the session commands
//! `elements.assign`, `elements.templates` and `elements.move` do.
//! `GetElementState`/`SetElementState`/`ClearElementState` map the states
//! the port models and report `False` for the view states it does not (see
//! [`element_state`]).

use std::cell::RefCell;
use std::rc::Rc;

use xedit_core::implementation::write::ElementState;
use xedit_core::implementation::{moving, template};
use xedit_core::interface::types::PascalEnum;
use xedit_core::interface::{Element, ElementType};
use xedit_script::adapter::{Adapter, Args};
use xedit_script::values::{ClassDef, Value, ValueError, VarType, to_bool};

use super::{add, boolean_at, element_at, integer_at, params, string_at, value_param};
use crate::script::ScriptRunState;

/// `IntToEsState` (`xejviScriptAdapterElement.pas:34`): the state index a
/// script names. `TwbElement.SetElementState` includes or excludes exactly
/// that bit, and a read answers whether it is in the element's set, so the
/// port models every state the mapping names, the view's own included
/// (`esHidden` and the sort keys): the `el-states` probe walks 0..15 on a
/// file, a record and a subrecord and compares every bit. The `else` arm is
/// `esDummy`, which is a real bit a script can set and read (upstream maps
/// every other index to it).
fn element_state(index: i64) -> ElementState {
    match index {
        0 => ElementState::esModified,
        1 => ElementState::esInternalModified,
        2 => ElementState::esUnsaved,
        3 => ElementState::esSortKeyValid,
        4 => ElementState::esExtendedSortKeyValid,
        5 => ElementState::esHidden,
        6 => ElementState::esParentHidden,
        7 => ElementState::esParentHiddenChecked,
        8 => ElementState::esNotReachable,
        9 => ElementState::esReachable,
        10 => ElementState::esTagged,
        11 => ElementState::esResolving,
        12 => ElementState::esNotSuitableToAddTo,
        _ => ElementState::esDummy,
    }
}

pub(crate) fn register(adapter: &mut Adapter, state: &Rc<RefCell<ScriptRunState>>, strings: &Rc<ClassDef>) {
    let _ = strings;

    //=== IwbElement =========================================================

    // `AssignTemplateByIndex`/`ByName`/`AssignTemplateCount`: the templates
    // of `IwbElement.GetAssignTemplates` at the index.
    add(
        adapter,
        "AssignTemplateCount",
        2,
        params(&[VarType::Empty, VarType::Integer]),
        |result, args| {
            if let Some(element) = element_at(args, 0) {
                let index = integer_at(args, 1)? as i32;
                *result = Value::Integer(template::assign_templates(&element, index).len() as i32);
            }
            Ok(())
        },
    );
    add(
        adapter,
        "AssignTemplateByIndex",
        3,
        params(&[VarType::Empty, VarType::Integer, VarType::Integer]),
        {
            let state = state.clone();
            move |result, args| {
                let index = integer_at(args, 1)? as i32;
                let template_index = integer_at(args, 2)?;
                if let Some(element) = element_at(args, 0) {
                    let templates = template::assign_templates(&element, index);
                    if let Some(found) = usize::try_from(template_index).ok().and_then(|at| templates.get(at)) {
                        *result = state.borrow().element_value(found);
                    }
                }
                Ok(())
            }
        },
    );
    add(
        adapter,
        "AssignTemplateByName",
        3,
        params(&[VarType::Empty, VarType::Integer, VarType::Str]),
        {
            let state = state.clone();
            move |result, args| {
                let index = integer_at(args, 1)? as i32;
                let name = string_at(args, 2);
                if let Some(element) = element_at(args, 0) {
                    let templates = template::assign_templates(&element, index);
                    if let Some(found) = templates
                        .iter()
                        .find(|candidate| candidate.get_name().eq_ignore_ascii_case(&name))
                    {
                        *result = state.borrow().element_value(found);
                    }
                }
                Ok(())
            }
        },
    );
    add(adapter, "BaseName", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0) {
            *result = Value::Str(element.get_base_name().into_bytes());
        }
        Ok(())
    });
    add(adapter, "BeginUpdate", 1, value_param(), |result, args| {
        if let Some(element) = element_at(args, 0) {
            *result = Value::Integer(element.begin_update_value());
        }
        Ok(())
    });
    add(adapter, "BuildRef", 1, value_param(), |_result, args| {
        // `IwbElement.BuildRef` is `TwbMainRecord.BuildRef` for a record and
        // does nothing for every other element.
        if let Some(element) = element_at(args, 0)
            && let Some(record) = element.as_element_impl().and_then(|element| element.main_record_impl())
        {
            record.build_ref();
        }
        Ok(())
    });
    add(adapter, "CanContainFormIDs", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(element) = element_at(args, 0)
            && let Some(implemented) = element.as_element_impl()
        {
            *result = Value::Bool(implemented.can_contain_form_ids());
        }
        Ok(())
    });
    // `CanMoveDown`/`CanMoveUp`/`MoveDown`/`MoveUp`: an entry of an
    // unsorted array one place down or up (`TwbElement.MoveUp`).
    add(adapter, "CanMoveDown", 1, value_param(), |result, args| {
        *result = Value::Bool(element_at(args, 0).is_some_and(|element| moving::can_move_down(&element)));
        Ok(())
    });
    add(adapter, "CanMoveUp", 1, value_param(), |result, args| {
        *result = Value::Bool(element_at(args, 0).is_some_and(|element| moving::can_move_up(&element)));
        Ok(())
    });
    add(adapter, "MoveDown", 1, value_param(), |_result, args| {
        if let Some(element) = element_at(args, 0) {
            moving::move_down(&element);
        }
        Ok(())
    });
    add(adapter, "MoveUp", 1, value_param(), |_result, args| {
        if let Some(element) = element_at(args, 0) {
            moving::move_up(&element);
        }
        Ok(())
    });
    add(adapter, "Check", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0) {
            *result = Value::Str(element.get_check().into_bytes());
        }
        Ok(())
    });
    for (name, clear) in [("ClearElementState", true), ("SetElementState", false)] {
        add(
            adapter,
            name,
            2,
            params(&[VarType::Empty, VarType::Empty]),
            move |result, args| {
                *result = Value::Str(Vec::new());
                if let Some(element) = element_at(args, 0)
                    && let Some(implemented) = element.as_element_impl()
                {
                    let base = implemented.element_base();
                    let state = element_state(integer_at(args, 1)?);
                    let had = base.has_state(state);
                    *result = Value::Bool(had);
                    base.set_state(state, !clear);
                }
                Ok(())
            },
        );
    }
    add(
        adapter,
        "GetElementState",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            *result = Value::Str(Vec::new());
            if let Some(element) = element_at(args, 0)
                && let Some(implemented) = element.as_element_impl()
            {
                let state = element_state(integer_at(args, 1)?);
                *result = Value::Bool(implemented.element_base().has_state(state));
            }
            Ok(())
        },
    );
    add(adapter, "ContainingMainRecord", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(element) = element_at(args, 0)
                && let Some(record) = element.get_containing_main_record()
            {
                *result = state
                    .borrow()
                    .element_value(&(record as xedit_core::interface::ElementRef));
            }
            Ok(())
        }
    });
    add(adapter, "ContainingSubRecord", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(element) = element_at(args, 0)
                && let Some(sub_record) = element.get_containing_sub_record()
            {
                *result = state.borrow().element_value(&sub_record);
            }
            Ok(())
        }
    });
    add(adapter, "DefType", 1, value_param(), |result, args| {
        *result = Value::Integer(-1);
        if let Some(element) = element_at(args, 0)
            && let Some(def) = def_type_of(&element)
        {
            *result = Value::Integer(def as i32);
        }
        Ok(())
    });
    add(adapter, "DefTypeAsText", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0)
            && let Some(def) = def_type_of(&element)
        {
            *result = Value::Str(def.enum_name().as_bytes().to_vec());
        }
        Ok(())
    });
    add(adapter, "DisplayName", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0) {
            *result = Value::Str(element.get_display_name(true).into_bytes());
        }
        Ok(())
    });
    // `ElementAssign`: `Element.Assign(Args.Values[1], Element2, Args.Values[3])`
    // with `Element2` the element of `Args.Values[2]` or nil.
    add(
        adapter,
        "ElementAssign",
        4,
        params(&[VarType::Empty, VarType::Empty, VarType::Empty, VarType::Bool]),
        {
            let state = state.clone();
            move |result, args| {
                let Some(element) = element_at(args, 0) else {
                    return Ok(());
                };
                let index = integer_at(args, 1)? as i32;
                // `if (V2O(Args.Values[2]) = nil) or Supports(...)`.
                // `nil` is `Value::Pointer(0)` (`GetConst`), `Null` and an
                // omitted argument count as nil too.
                let source_is_nil = args
                    .values
                    .get(2)
                    .is_none_or(|value| value.is_nil() || matches!(value, Value::Empty));
                let source_ok = source_is_nil || element_at(args, 2).is_some();
                if !source_ok {
                    return Ok(());
                }
                let source = element_at(args, 2);
                let only_sk = boolean_at(args, 3)?;
                if let Some(assigned) = element.assign(index, source.as_ref(), only_sk) {
                    *result = state.borrow().element_value(&assigned);
                }
                Ok(())
            }
        },
    );
    add(adapter, "ElementType", 1, value_param(), |result, args| {
        *result = Value::Integer(-1);
        if let Some(element) = element_at(args, 0) {
            *result = Value::Integer(element.get_element_type().ord() as i32);
        }
        Ok(())
    });
    add(adapter, "ElementTypeAsText", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0) {
            *result = Value::Str(element.get_element_type().enum_name().as_bytes().to_vec());
        }
        Ok(())
    });
    add(adapter, "EndUpdate", 1, value_param(), |result, args| {
        if let Some(element) = element_at(args, 0) {
            *result = Value::Integer(element.end_update_value());
        }
        Ok(())
    });
    add(adapter, "EnumValues", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        let Some(element) = element_at(args, 0) else {
            return Ok(());
        };
        let Some(value_def) = element.get_value_def() else {
            return Ok(());
        };
        let Some(integer_def) = value_def.as_integer_def() else {
            return Ok(());
        };
        let Some(formater) = integer_def.get_formater(Some(&element)) else {
            return Ok(());
        };
        let Some(enums) = formater.as_enum_def() else {
            return Ok(());
        };
        let names: Vec<String> = (0..enums.get_name_count())
            .map(|index| enums.get_name_of(i64::from(index)))
            .collect();
        *result = Value::Str(names.join("\r\n").into_bytes());
        Ok(())
    });
    add(
        adapter,
        "Equals",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            *result = Value::Bool(false);
            if let Some(element) = element_at(args, 0)
                && let Some(other) = element_at(args, 1)
            {
                *result = Value::Bool(element.get_element_id() == other.get_element_id());
            }
            Ok(())
        },
    );
    add(adapter, "FlagValues", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        let Some(element) = element_at(args, 0) else {
            return Ok(());
        };
        let Some(value_def) = element.get_value_def() else {
            return Ok(());
        };
        let Some(integer_def) = value_def.as_integer_def() else {
            return Ok(());
        };
        let Some(formater) = integer_def.get_formater(Some(&element)) else {
            return Ok(());
        };
        let Some(flags) = formater.as_flags_def() else {
            return Ok(());
        };
        let names: Vec<String> = (0..flags.get_flag_count())
            .map(|index| flags.get_flag(index as usize, false))
            .collect();
        *result = Value::Str(names.join("\r\n").into_bytes());
        Ok(())
    });
    add(adapter, "FullPath", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0) {
            *result = Value::Str(element.get_full_path().into_bytes());
        }
        Ok(())
    });
    add(adapter, "GetContainer", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(element) = element_at(args, 0)
                && let Some(container) = element.get_container()
            {
                *result = state.borrow().element_value(&container);
            }
            Ok(())
        }
    });
    add(adapter, "GetEditValue", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0) {
            // `TwbMainRecord.GetEditValue` is the FormID as text, where the
            // port's element trait keeps the empty value of a container; the
            // other elements give their own edit value.
            let text = if element.get_element_type() == ElementType::etMainRecord {
                edit_value_of_record(&element)
            } else {
                element.get_edit_value()
            };
            *result = Value::Str(text.into_bytes());
        }
        Ok(())
    });
    add(adapter, "GetFile", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(element) = element_at(args, 0)
                && let Some(file) = element.get_file()
            {
                *result = state
                    .borrow()
                    .element_value(&(file as xedit_core::interface::ElementRef));
            }
            Ok(())
        }
    });
    add(adapter, "GetNativeValue", 1, value_param(), |result, args| {
        *result = Value::Null;
        if let Some(element) = element_at(args, 0) {
            *result = super::native_to_value(element.get_native_value());
        }
        Ok(())
    });
    add(adapter, "GetSummary", 1, value_param(), |result, args| {
        if let Some(element) = element_at(args, 0) {
            *result = Value::Str(element.get_summary().into_bytes());
        }
        Ok(())
    });
    add(adapter, "GetValue", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0) {
            *result = Value::Str(element.get_value().into_bytes());
        }
        Ok(())
    });
    add(
        adapter,
        "IndexedPath",
        2,
        params(&[VarType::Empty, VarType::Bool]),
        |result, args| {
            *result = Value::Str(Vec::new());
            if let Some(element) = element_at(args, 0) {
                let from_file = boolean_at(args, 1)?;
                *result = Value::Str(indexed_path(&element, from_file).into_bytes());
            }
            Ok(())
        },
    );
    add(adapter, "IsEditable", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(element) = element_at(args, 0) {
            *result = Value::Bool(element.get_is_editable());
        }
        Ok(())
    });
    add(adapter, "IsInjected", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(element) = element_at(args, 0)
            && let Some(record) = element.into_main_record()
        {
            *result = Value::Bool(record.get_is_injected());
        }
        Ok(())
    });
    add(adapter, "LinksTo", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(element) = element_at(args, 0)
                && let Some(linked) = element.get_links_to()
            {
                *result = state.borrow().element_value(&linked);
            }
            Ok(())
        }
    });
    add(adapter, "MarkModifiedRecursive", 1, value_param(), |_result, args| {
        if let Some(element) = element_at(args, 0) {
            element.mark_modified_recursive();
        }
        Ok(())
    });
    add(adapter, "Path", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0) {
            *result = Value::Str(element.get_path().into_bytes());
        }
        Ok(())
    });
    add(adapter, "PathName", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0) {
            *result = Value::Str(path_name(&element).into_bytes());
        }
        Ok(())
    });
    add(adapter, "Remove", 1, value_param(), |_result, args| {
        if let Some(element) = element_at(args, 0) {
            element.remove();
        }
        Ok(())
    });
    add(
        adapter,
        "ReportRequiredMasters",
        4,
        params(&[VarType::Empty, VarType::Empty, VarType::Empty, VarType::Empty]),
        |_result, args| {
            let Some(element) = element_at(args, 0) else {
                return Ok(());
            };
            let Some(object) = super::strings_at(args, 1) else {
                return Ok(());
            };
            let as_new = to_bool(args.values.get(2).unwrap_or(&Value::Empty)).unwrap_or(false);
            // `Element.ReportRequiredMasters(lMasters, aAsNew, recursive)`
            // with the `recursive` argument of the call (the Pascal passes
            // `Args.Values[3]`); `lStrings.AddObject(lFile.FileName,
            // Pointer(lFile))`.
            let recursive = to_bool(args.values.get(3).unwrap_or(&Value::Empty)).unwrap_or(false);
            let masters = match element.as_element_impl() {
                Some(implemented) => {
                    let mut set = xedit_core::implementation::copy::FilesSet::new();
                    implemented.report_required_masters(&mut set, as_new, recursive, false);
                    set.files().to_vec()
                }
                None => Vec::new(),
            };
            if let Some(list) = object.borrow_mut().string_list_mut() {
                for file in masters {
                    let name = file.get_name();
                    list.add_object(name.into_bytes(), Value::Empty);
                }
            }
            Ok(())
        },
    );
    add(
        adapter,
        "SetEditValue",
        2,
        params(&[VarType::Empty, VarType::Str]),
        |_result, args| {
            if let Some(element) = element_at(args, 0) {
                let value = string_at(args, 1);
                // Upstream `Element.EditValue := value` raises a bare
                // `Exception` on a refusal, which the run line prints as
                // `[Exception]`.
                if let Err(message) = element.set_edit_value(&value) {
                    // A main record's FormID text that does not parse is the
                    // RTL's `EConvertError` (`StrToInt64('$' + value)`).
                    let class = if message.ends_with("' is not a valid integer value") {
                        "EConvertError"
                    } else {
                        "Exception"
                    };
                    return Err(ValueError::external_class(class, message));
                }
            }
            Ok(())
        },
    );
    add(
        adapter,
        "SetNativeValue",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |_result, args| {
            if let Some(element) = element_at(args, 0) {
                let value = super::value_to_native(args.values.get(1).unwrap_or(&Value::Empty))?;
                // Upstream `Element.NativeValue := value` raises a bare
                // `Exception` on a refusal.
                if let Err(message) = element.set_native_value(value) {
                    return Err(ValueError::external_class("Exception", message));
                }
            }
            Ok(())
        },
    );
    add(adapter, "SetToDefault", 1, value_param(), |_result, args| {
        if let Some(element) = element_at(args, 0) {
            super::refused(element.set_to_default())?;
        }
        Ok(())
    });
    add(adapter, "ShortName", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0) {
            *result = Value::Str(short_name(&element).into_bytes());
        }
        Ok(())
    });
    add(
        adapter,
        "SortKey",
        2,
        params(&[VarType::Empty, VarType::Bool]),
        |result, args| {
            *result = Value::Str(Vec::new());
            if let Some(element) = element_at(args, 0) {
                let extended = boolean_at(args, 1)?;
                *result = Value::Str(element.get_sort_key(extended).into_bytes());
            }
            Ok(())
        },
    );
    // `TemplateAssign`: the template of the name at `High(Integer)`
    // (`wbAssignAdd`), assigned there, the element made empty set to its
    // default.
    add(adapter, "TemplateAssign", 2, params(&[VarType::Empty, VarType::Str]), {
        let state = state.clone();
        move |result, args| {
            let Some(element) = element_at(args, 0) else {
                return Ok(());
            };
            let name = string_at(args, 1);
            let index = xedit_core::interface::types::ASSIGN_ADD;
            let templates = template::assign_templates(&element, index);
            if let Some(found) = templates
                .iter()
                .find(|candidate| name.eq_ignore_ascii_case(&candidate.get_name()))
            {
                let assigned = element.assign(index, Some(found), false);
                if let Some(assigned) = &assigned {
                    template::set_to_default_if_as_created_empty(assigned).map_err(ValueError::External)?;
                    *result = state.borrow().element_value(assigned);
                } else {
                    *result = Value::Empty;
                }
            }
            Ok(())
        }
    });

    //=== the `wbCopyElementTo*` helpers =====================================

    add(
        adapter,
        "wbCopyElementToFile",
        4,
        params(&[VarType::Empty, VarType::Empty, VarType::Bool, VarType::Bool]),
        {
            let state = state.clone();
            move |result, args| copy_to_file(&state, result, args, CopyNames::default())
        },
    );
    add(
        adapter,
        "wbCopyElementToFileWithPrefix",
        7,
        params(&[
            VarType::Empty,
            VarType::Empty,
            VarType::Bool,
            VarType::Bool,
            VarType::Empty,
            VarType::Empty,
            VarType::Empty,
        ]),
        {
            let state = state.clone();
            move |result, args| {
                copy_to_file(
                    &state,
                    result,
                    args,
                    CopyNames {
                        prefix: Some(4),
                        prefix_remove: Some(5),
                        suffix: Some(6),
                        ..CopyNames::default()
                    },
                )
            }
        },
    );
    add(
        adapter,
        "wbCopyElementToFileWithPrefixAndSuffix",
        8,
        params(&[
            VarType::Empty,
            VarType::Empty,
            VarType::Bool,
            VarType::Bool,
            VarType::Empty,
            VarType::Empty,
            VarType::Empty,
            VarType::Empty,
        ]),
        {
            let state = state.clone();
            move |result, args| {
                copy_to_file(
                    &state,
                    result,
                    args,
                    CopyNames {
                        prefix: Some(4),
                        suffix: Some(5),
                        prefix_remove: Some(6),
                        suffix_remove: Some(7),
                    },
                )
            }
        },
    );
    add(
        adapter,
        "wbCopyElementToRecord",
        4,
        params(&[VarType::Empty, VarType::Empty, VarType::Bool, VarType::Bool]),
        {
            let state = state.clone();
            move |result, args| {
                let (Some(element), Some(record)) = (element_at(args, 0), super::main_record_at(args, 1)) else {
                    return Ok(());
                };
                let as_new = boolean_at(args, 2)?;
                let deep = boolean_at(args, 3)?;
                let Some(record) = record.as_element_impl().and_then(|element| element.main_record_impl()) else {
                    return Ok(());
                };
                if let Ok(Some(copied)) =
                    xedit_core::implementation::copy::copy_element_to_record(&element, &record, as_new, deep)
                {
                    *result = state.borrow().element_value(&copied);
                }
                Ok(())
            }
        },
    );

    let _ = strings;
}

/// The argument positions of the `wbCopyElementToFileWithPrefix*` names.
#[derive(Default)]
struct CopyNames {
    prefix: Option<usize>,
    suffix: Option<usize>,
    prefix_remove: Option<usize>,
    suffix_remove: Option<usize>,
}

fn copy_to_file(
    state: &Rc<RefCell<ScriptRunState>>,
    result: &mut Value,
    args: &mut Args,
    names: CopyNames,
) -> Result<(), xedit_script::values::ValueError> {
    let (Some(element), Some(file)) = (element_at(args, 0), super::file_at(args, 1)) else {
        return Ok(());
    };
    let as_new = boolean_at(args, 2)?;
    let deep = boolean_at(args, 3)?;
    let text = |index: Option<usize>| index.map(|index| string_at(args, index)).unwrap_or_default();
    let copy_args = xedit_core::interface::CopyArgs {
        as_new,
        deep_copy: deep,
        prefix_remove: text(names.prefix_remove),
        suffix_remove: text(names.suffix_remove),
        prefix: text(names.prefix),
        suffix: text(names.suffix),
        // `aAllowOverwrite` is False for all three registrations.
        allow_overwrite: false,
    };
    let Some(file) = file.as_element_impl().and_then(|element| element.file_impl()) else {
        return Ok(());
    };
    if let Ok(Some(copied)) = xedit_core::implementation::copy::copy_element_to_file(&element, &file, &copy_args) {
        *result = state.borrow().element_value(&copied);
    }
    Ok(())
}

/// `IwbElement.ValueDef.DefType`: the value definition of the element, or
/// the named definition where there is none (a main record's definition is
/// its value definition, so `DefType(e)` of a record is `dtRecord`).
fn def_type_of(element: &xedit_core::interface::ElementRef) -> Option<xedit_core::interface::DefType> {
    element
        .get_value_def()
        .map(|def| def.get_def_type())
        .or_else(|| element.get_def().map(|def| def.get_def_type()))
}

/// `TwbMainRecord.GetEditValue`: the load order FormID, else the FormID, as
/// text (`ToString(False)`).
fn edit_value_of_record(element: &xedit_core::interface::ElementRef) -> String {
    let Some(record) = element.clone().into_main_record() else {
        return String::new();
    };
    let form_id = if xedit_core::interface::globals::display_load_order_form_id() {
        record.get_load_order_form_id()
    } else {
        record.get_form_id()
    };
    form_id.to_string(false)
}

/// `ShortName`: the editor ID or FormID of a record (`TwbMainRecord`), the
/// file name of a file, the name of every other element.
fn short_name(element: &xedit_core::interface::ElementRef) -> String {
    match element.get_element_type() {
        ElementType::etMainRecord => element
            .as_main_record()
            .map(|record| record.get_short_name())
            .unwrap_or_default(),
        ElementType::etFile => element.get_name(),
        _ => element.get_name(),
    }
}

/// `PathName` (`wbImplementation.pas:19653`), the composition of the
/// container's path name and this element's position and short name.
fn path_name(element: &xedit_core::interface::ElementRef) -> String {
    let Some(container) = element.get_container() else {
        return format!(" \\ {}", short_name(element));
    };
    let position = container
        .as_container()
        .map(|container| container.index_of_id(element.get_element_id()))
        .unwrap_or(-1);
    format!("{} \\ [{}] {}", path_name(&container), position, short_name(element))
}

/// `IndexedPath` (`wbImplementation.pas:19625`).
fn indexed_path(element: &xedit_core::interface::ElementRef, index_from_file: bool) -> String {
    let Some(container) = element.get_container() else {
        return String::new();
    };
    let mut result = indexed_path(&container, index_from_file);
    if !index_from_file
        && matches!(
            container.get_element_type(),
            ElementType::etFile | ElementType::etGroupRecord
        )
    {
        return result;
    }
    let position = container
        .as_container()
        .map(|container| container.index_of_id(element.get_element_id()))
        .unwrap_or(-1);
    if !result.is_empty() {
        result.push('\\');
    }
    result.push_str(&format!("[{position}]"));
    result
}
