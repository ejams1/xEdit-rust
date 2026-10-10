// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterRecord.pas

//! `xejviScriptAdapterRecord.pas`: the `IwbMainRecord` and `IwbGroupRecord`
//! functions over `xedit_core`'s main records and groups.
//!
//! The reference half (`ReferencedBy*`, `ReferencesByIndex`/`Count`) reads
//! the reference index of phase 4 through `xedit_core::implementation::refs`;
//! the session command behind it is `refs.get`. `GetGridCell`, `GetPosition`
//! and `GetRotation` hand back the `TwbGridCell`/`TwbVector` record values
//! (`super::record_def`).

use std::cell::RefCell;
use std::rc::Rc;

use xedit_core::interface::{Element, ElementRef, FormID, MainRecordRef};
use xedit_script::adapter::{Adapter, Args};
use xedit_script::values::{Value, VarType, to_bool};

use super::{add, boolean_at, element_at, integer_at, main_record_at, params, string_at, value_param};
use crate::script::ScriptRunState;

pub(crate) fn register(adapter: &mut Adapter, state: &Rc<RefCell<ScriptRunState>>) {
    let vector = super::record_def(adapter, "TwbVector");
    let grid_cell = super::record_def(adapter, "TwbGridCell");

    //=== IwbMainRecord ======================================================

    add(adapter, "BaseRecord", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some(base) = record.get_base_record()
            {
                *result = state.borrow().element_value(&(base as ElementRef));
            }
            Ok(())
        }
    });
    add(adapter, "BaseRecordID", 1, value_param(), |result, args| {
        if let Some(record) = main_record_at(args, 0)
            && let Some(base) = record.get_base_record()
        {
            *result = Value::Int64(i64::from(base.get_form_id().to_cardinal()));
        }
        Ok(())
    });
    add(
        adapter,
        "ChangeFormSignature",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |_result, args| {
            if let Some(record) = main_record_at(args, 0) {
                record.change_form_signature(super::signature_at(args, 1));
            }
            Ok(())
        },
    );
    add(adapter, "ChildGroup", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some(group) = record
                    .as_element_impl()
                    .and_then(|element| element.main_record_impl())
                    .and_then(|record| record.child_group())
            {
                *result = state.borrow().element_value(&(group as ElementRef));
            }
            Ok(())
        }
    });
    add(
        adapter,
        "CompareExchangeFormID",
        3,
        params(&[VarType::Empty, VarType::Empty, VarType::Empty]),
        |result, args| {
            *result = Value::Bool(false);
            if let Some(record) = main_record_at(args, 0) {
                let old = form_id_at(args, 1);
                let new = form_id_at(args, 2);
                if let Some(implemented) = record.as_element_impl()
                    && let Some(record) = implemented.main_record_impl()
                    && let Ok(exchanged) = record.compare_exchange_form_id(old, new)
                {
                    *result = Value::Bool(exchanged);
                }
            }
            Ok(())
        },
    );
    add(adapter, "EditorID", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Str(record.get_editor_id().into_bytes());
        }
        Ok(())
    });
    add(adapter, "FixedFormID", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Int64(i64::from(record.get_fixed_form_id().to_cardinal()));
        }
        Ok(())
    });
    add(adapter, "FormID", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Int64(i64::from(record.get_form_id().to_cardinal()));
        }
        Ok(())
    });
    add(adapter, "GetFormVCS1", 1, value_param(), |result, args| {
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Int64(i64::from(record.get_vcs1()));
        }
        Ok(())
    });
    add(adapter, "GetFormVCS2", 1, value_param(), |result, args| {
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Int64(i64::from(record.get_vcs2()));
        }
        Ok(())
    });
    add(adapter, "GetFormVersion", 1, value_param(), |result, args| {
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Int64(i64::from(record.get_version()));
        }
        Ok(())
    });
    add(
        adapter,
        "SetFormVCS1",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |_result, args| {
            if let Some(record) = main_record_at(args, 0) {
                record.set_vcs1(integer_at(args, 1).unwrap_or(0) as u32);
            }
            Ok(())
        },
    );
    add(
        adapter,
        "SetFormVCS2",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |_result, args| {
            if let Some(record) = main_record_at(args, 0) {
                record.set_vcs2(integer_at(args, 1).unwrap_or(0) as u32);
            }
            Ok(())
        },
    );
    add(
        adapter,
        "SetFormVersion",
        2,
        params(&[VarType::Empty, VarType::Integer]),
        |_result, args| {
            if let Some(record) = main_record_at(args, 0) {
                record.set_version(integer_at(args, 1)? as u32);
            }
            Ok(())
        },
    );
    add(
        adapter,
        "SetEditorID",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |_result, args| {
            if let Some(record) = main_record_at(args, 0) {
                // `MainRecord.EditorID := Args.Values[1]`, the record's editor ID
                // written through its definition's callback.
                let value = string_at(args, 1);
                if let Some(implemented) = record.as_element_impl()
                    && let Some(record) = implemented.main_record_impl()
                    && let Some(def) = record.def()
                    && let Some(container) = record.as_container()
                    && let Some(sub_record) =
                        container.get_element_by_signature(xedit_core::interface::Signature::new(b"EDID"))
                    && let Err(_error) = def.set_editor_id(&sub_record, &value)
                {
                    // The definition callback reports its own failure upstream;
                    // the port leaves the value as it was.
                }
            }
            Ok(())
        },
    );
    add(adapter, "GetGridCell", 1, value_param(), {
        let grid_cell = grid_cell.clone();
        move |result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some((x, y)) = record.get_grid_cell()
            {
                *result = super::grid_cell_value(grid_cell.clone(), x, y);
            }
            Ok(())
        }
    });
    add(adapter, "GetPosition", 1, value_param(), {
        let vector = vector.clone();
        move |result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some((x, y, z)) = position_of(&record)
            {
                *result = super::vector_value(vector.clone(), x, y, z);
            }
            Ok(())
        }
    });
    add(adapter, "GetRotation", 1, value_param(), {
        let vector = vector.clone();
        move |result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some((x, y, z)) = rotation_of(&record)
            {
                *result = super::vector_value(vector.clone(), x, y, z);
            }
            Ok(())
        }
    });
    add(adapter, "GetIsDeleted", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Bool(record.get_is_deleted());
        }
        Ok(())
    });
    add(adapter, "GetIsInitiallyDisabled", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(record) = main_record_at(args, 0)
            && let Some(implemented) = record.as_element_impl()
            && let Some(record) = implemented.main_record_impl()
        {
            *result = Value::Bool(record.get_is_initially_disabled());
        }
        Ok(())
    });
    add(adapter, "GetIsPersistent", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Bool(record.get_is_persistent());
        }
        Ok(())
    });
    add(adapter, "GetIsVisibleWhenDistant", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Bool(record.get_is_visible_when_distant());
        }
        Ok(())
    });
    add(adapter, "GetLoadOrderFormID", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Int64(i64::from(record.get_load_order_form_id().to_cardinal()));
        }
        Ok(())
    });
    add(
        adapter,
        "SetLoadOrderFormID",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |_result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some(implemented) = record.as_element_impl()
                && let Some(record) = implemented.main_record_impl()
            {
                let _ = record.set_load_order_form_id(form_id_at(args, 1));
            }
            Ok(())
        },
    );
    add(adapter, "HasPrecombinedMesh", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Bool(record.get_has_precombined_mesh());
        }
        Ok(())
    });
    add(adapter, "PrecombinedMesh", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Str(record.get_precombined_mesh().into_bytes());
        }
        Ok(())
    });
    add(
        adapter,
        "HighestOverrideOrSelf",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        {
            let state = state.clone();
            move |result, args| {
                if let Some(record) = main_record_at(args, 0) {
                    let max_load_order = integer_at(args, 1)? as i32;
                    let highest = record.get_highest_override_or_self(max_load_order);
                    *result = state.borrow().element_value(&(highest as ElementRef));
                }
                Ok(())
            }
        },
    );
    add(adapter, "IsMaster", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(record) = main_record_at(args, 0) {
            *result = Value::Bool(record.get_is_master());
        }
        Ok(())
    });
    add(adapter, "IsWinningOverride", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(record) = main_record_at(args, 0) {
            // `Result := Self = WinningOverride`.
            let winning = record.get_winning_override();
            *result = Value::Bool(winning.get_element_id() == record.get_element_id());
        }
        Ok(())
    });
    add(adapter, "Master", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some(master) = record
                    .as_element_impl()
                    .and_then(|element| element.main_record_impl())
                    .and_then(|record| record.master())
            {
                *result = state.borrow().element_value(&(master as ElementRef));
            }
            Ok(())
        }
    });
    add(adapter, "MasterOrSelf", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(record) = main_record_at(args, 0) {
                let master = record.get_master_or_self();
                *result = state.borrow().element_value(&(master as ElementRef));
            }
            Ok(())
        }
    });
    add(
        adapter,
        "OverrideByIndex",
        2,
        params(&[VarType::Empty, VarType::Integer]),
        {
            let state = state.clone();
            move |result, args| {
                if let Some(record) = main_record_at(args, 0)
                    && let Some(implemented) = record.as_element_impl()
                    && let Some(record) = implemented.main_record_impl()
                {
                    let index = integer_at(args, 1)? as i32;
                    if let Some(override_record) = usize::try_from(index)
                        .ok()
                        .and_then(|index| record.overrides().get(index).cloned())
                    {
                        *result = state.borrow().element_value(&(override_record as ElementRef));
                    }
                }
                Ok(())
            }
        },
    );
    add(adapter, "OverrideCount", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        if let Some(record) = main_record_at(args, 0)
            && let Some(implemented) = record.as_element_impl()
            && let Some(record) = implemented.main_record_impl()
        {
            *result = Value::Integer(record.overrides().len() as i32);
        }
        Ok(())
    });
    add(adapter, "ReferencedByCount", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        if let Some(record) = main_record_at(args, 0)
            && let Some(implemented) = record.as_element_impl()
            && let Some(record) = implemented.main_record_impl()
        {
            *result = Value::Integer(record.referenced_by_count() as i32);
        }
        Ok(())
    });
    add(
        adapter,
        "ReferencedByIndex",
        2,
        params(&[VarType::Empty, VarType::Integer]),
        {
            let state = state.clone();
            move |result, args| {
                if let Some(record) = main_record_at(args, 0)
                    && let Some(implemented) = record.as_element_impl()
                    && let Some(record) = implemented.main_record_impl()
                {
                    let index = integer_at(args, 1)? as i32;
                    if let Some(referencer) = usize::try_from(index)
                        .ok()
                        .and_then(|index| record.referenced_by().get(index).cloned())
                    {
                        *result = state.borrow().element_value(&(referencer as ElementRef));
                    }
                }
                Ok(())
            }
        },
    );
    add(
        adapter,
        "ReferencesByIndex",
        2,
        params(&[VarType::Empty, VarType::Integer]),
        {
            let state = state.clone();
            move |result, args| {
                if let Some(record) = main_record_at(args, 0)
                    && let Some(implemented) = record.as_element_impl()
                    && let Some(record) = implemented.main_record_impl()
                {
                    let index = integer_at(args, 1)? as i32;
                    let form_id = usize::try_from(index)
                        .ok()
                        .and_then(|index| record.references().get(index).copied());
                    if let Some(form_id) = form_id
                        && let Some(reference) = xedit_core::interface::record_by_load_order_form_id(
                            form_id,
                            record
                                .record_file()
                                .map(|file| file as xedit_core::interface::FileRef)
                                .as_ref(),
                        )
                    {
                        *result = state.borrow().element_value(&(reference as ElementRef));
                    }
                }
                Ok(())
            }
        },
    );
    add(adapter, "ReferencesCount", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        if let Some(record) = main_record_at(args, 0)
            && let Some(implemented) = record.as_element_impl()
            && let Some(record) = implemented.main_record_impl()
        {
            *result = Value::Integer(record.references().len() as i32);
        }
        Ok(())
    });
    add(
        adapter,
        "SetIsDeleted",
        2,
        params(&[VarType::Empty, VarType::Bool]),
        |_result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some(implemented) = record.as_element_impl()
                && let Some(record) = implemented.main_record_impl()
            {
                let value = boolean_at(args, 1)?;
                let _ = record.set_is_deleted(value);
            }
            Ok(())
        },
    );
    add(
        adapter,
        "SetIsInitiallyDisabled",
        2,
        params(&[VarType::Empty, VarType::Bool]),
        |_result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some(implemented) = record.as_element_impl()
                && let Some(record) = implemented.main_record_impl()
            {
                let value = boolean_at(args, 1)?;
                record.set_is_initially_disabled(value);
            }
            Ok(())
        },
    );
    add(
        adapter,
        "SetIsPersistent",
        2,
        params(&[VarType::Empty, VarType::Bool]),
        |_result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some(implemented) = record.as_element_impl()
                && let Some(record) = implemented.main_record_impl()
            {
                let value = boolean_at(args, 1)?;
                record.set_is_persistent(value);
            }
            Ok(())
        },
    );
    add(
        adapter,
        "SetIsVisibleWhenDistant",
        2,
        params(&[VarType::Empty, VarType::Bool]),
        |_result, args| {
            if let Some(record) = main_record_at(args, 0)
                && let Some(implemented) = record.as_element_impl()
                && let Some(record) = implemented.main_record_impl()
            {
                let value = boolean_at(args, 1)?;
                record.set_is_visible_when_distant(value);
            }
            Ok(())
        },
    );
    add(adapter, "Signature", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        if let Some(element) = element_at(args, 0)
            && let Some(signature) = element.get_has_signature()
        {
            *result = Value::Str(super::signature_text_of(signature).into_bytes());
        }
        Ok(())
    });
    add(adapter, "UpdateRefs", 1, value_param(), |_result, args| {
        if let Some(record) = main_record_at(args, 0)
            && let Some(implemented) = record.as_element_impl()
            && let Some(record) = implemented.main_record_impl()
        {
            record.update_refs();
        }
        Ok(())
    });
    add(adapter, "WinningOverride", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(record) = main_record_at(args, 0) {
                let winning = record.get_winning_override();
                *result = state.borrow().element_value(&(winning as ElementRef));
            }
            Ok(())
        }
    });

    //=== IwbGroupRecord =====================================================

    add(adapter, "ChildrenOf", 1, value_param(), {
        let state = state.clone();
        move |result, args| {
            if let Some(element) = element_at(args, 0)
                && let Some(group) = element
                    .as_element_impl()
                    .and_then(|element| element.group_record_impl())
                && let Some(record) = group.children_of()
            {
                *result = state.borrow().element_value(&(record as ElementRef));
            }
            Ok(())
        }
    });
    add(
        adapter,
        "FindChildGroup",
        3,
        params(&[VarType::Empty, VarType::Empty, VarType::Empty]),
        {
            let state = state.clone();
            move |result, args| {
                let Some(element) = element_at(args, 0) else {
                    return Ok(());
                };
                let Some(group) = element
                    .as_element_impl()
                    .and_then(|element| element.group_record_impl())
                else {
                    return Ok(());
                };
                let group_type = integer_at(args, 1)? as i32;
                // `FindChildGroup(aGroupType, aMainRecord)`:
                // `aMainRecord.FormID.ToCardinal`.
                let Some(label) = main_record_at(args, 2).map(|record| record.get_form_id().to_cardinal()) else {
                    return Ok(());
                };
                if let Some(found) = group.find_child_group(group_type, label) {
                    *result = state.borrow().element_value(&(found as ElementRef));
                }
                Ok(())
            }
        },
    );
    add(adapter, "GroupLabel", 1, value_param(), |result, args| {
        *result = Value::Integer(-1);
        if let Some(element) = element_at(args, 0)
            && let Some(group) = element
                .as_element_impl()
                .and_then(|element| element.group_record_impl())
        {
            *result = Value::Int64(i64::from(group.group_label()));
        }
        Ok(())
    });
    add(adapter, "GroupType", 1, value_param(), |result, args| {
        *result = Value::Integer(-1);
        if let Some(element) = element_at(args, 0)
            && let Some(group) = element
                .as_element_impl()
                .and_then(|element| element.group_record_impl())
        {
            *result = Value::Integer(group.group_type());
        }
        Ok(())
    });
    add(
        adapter,
        "MainRecordByEditorID",
        2,
        params(&[VarType::Empty, VarType::Str]),
        {
            let state = state.clone();
            move |result, args| {
                if let Some(element) = element_at(args, 0)
                    && let Some(group) = element
                        .as_element_impl()
                        .and_then(|element| element.group_record_impl())
                {
                    let editor_id = string_at(args, 1);
                    if let Some(record) = group.main_record_by_editor_id(&editor_id) {
                        *result = state.borrow().element_value(&(record as ElementRef));
                    }
                }
                Ok(())
            }
        },
    );

    let _ = to_bool;
}

/// `TwbFormID.FromVar(Args.Values[i])`: the FormID of a script value.
fn form_id_at(args: &Args, index: usize) -> FormID {
    FormID::from_cardinal(integer_at(args, index).unwrap_or(0) as u32)
}

/// `TwbMainRecord.GetPosition`: the `DATA` position of a placed record.
fn position_of(record: &MainRecordRef) -> Option<(f32, f32, f32)> {
    let (x, y, z) = record.as_element_impl()?.main_record_impl()?.get_position()?;
    Some((x as f32, y as f32, z as f32))
}

/// `TwbMainRecord.GetRotation`: the `DATA` rotation of a placed record.
fn rotation_of(record: &MainRecordRef) -> Option<(f32, f32, f32)> {
    let (x, y, z) = record.as_element_impl()?.main_record_impl()?.get_rotation()?;
    Some((x as f32, y as f32, z as f32))
}
