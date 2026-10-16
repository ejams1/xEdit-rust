// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterFile.pas

//! `xejviScriptAdapterFile.pas`: the `IwbFile` functions over
//! `xedit_core`'s file implementation, with the masters, the flags
//! (`files.flags`), the FormID conversions (`formids.*`) and the record
//! lookups of the phase-3/4 commands.
//!
//! `RecordByFormID` was recorded as step-4 owed (the `AdapterHost` list in
//! `docs/PLAN.md`); it is here, over
//! `File::get_record_by_form_id(form_id, new_masters, allow_injected)`, the
//! same function the `records.get` command reads through.
//!
//! `FileWriteToStream` writes the file into a `TStream` object of the
//! `Classes` unit (step 8's streams): `TwbFile.WriteToStream` with the
//! `TwbResetModified` the third argument names, the bytes the save writes
//! and the same flag resets. The `file-stream` probe compares the bytes, the
//! stream position and the flag states after each of the four routes.

use std::cell::RefCell;
use std::rc::Rc;

use xedit_core::implementation::ResetModified;
use xedit_core::interface::{ElementRef, FileRef, FileState, FormID};
use xedit_script::adapter::{Adapter, Args};
use xedit_script::values::{ClassDef, Value, ValueError, VarType};

use super::{
    add, boolean_at, check_arg_count, element_at, file_at, file_impl_of, integer_at, params, string_at, value_param,
};
use crate::script::ScriptRunState;

pub(crate) fn register(adapter: &mut Adapter, state: &Rc<RefCell<ScriptRunState>>, strings: &Rc<ClassDef>) {
    let _ = strings;

    //=== IwbFile ============================================================

    add(
        adapter,
        "AddMasterIfMissing",
        -1,
        params(&[VarType::Empty, VarType::Str]),
        |_result, args| {
            let Some(file) = file_at(args, 0) else {
                return Ok(());
            };
            // `case Args.Count of 0, 1: ieNotEnoughParams; 4: (name, sort,
            // silent); 3: (name, sort); 2: (name); else ieTooManyParams`.
            check_arg_count(args, 2, 4)?;
            let master = string_at(args, 1);
            let sort_masters = boolean_at(args, 2).unwrap_or(false);
            let silent = boolean_at(args, 3).unwrap_or(false);
            let Some(file) = file_impl_of(&file) else {
                return Ok(());
            };
            super::refused(file.add_master_if_missing(&master, sort_masters, silent))?;
            Ok(())
        },
    );
    add(
        adapter,
        "AddMasters",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |_result, args| {
            let Some(file) = file_at(args, 0) else {
                return Ok(());
            };
            // `case Args.Count of 0, 1: ieNotEnoughParams; 3: (list, sort);
            // 2: (list); else ieTooManyParams`.
            check_arg_count(args, 2, 3)?;
            let names = match super::strings_at(args, 1) {
                Some(object) => object
                    .borrow()
                    .string_list()
                    .map(|list| {
                        list.lines()
                            .iter()
                            .map(|line| String::from_utf8_lossy(line).into_owned())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                None => return Ok(()),
            };
            let Some(file) = file_impl_of(&file) else {
                return Ok(());
            };
            super::refused(file.add_masters(&names, false))?;
            Ok(())
        },
    );
    add(
        adapter,
        "AddMastersIfMissing",
        -1,
        params(&[VarType::Empty, VarType::Str]),
        |_result, args| {
            let Some(file) = file_at(args, 0) else {
                return Ok(());
            };
            check_arg_count(args, 2, 4)?;
            let names = match super::strings_at(args, 1) {
                Some(object) => object
                    .borrow()
                    .string_list()
                    .map(|list| {
                        list.lines()
                            .iter()
                            .map(|line| String::from_utf8_lossy(line).into_owned())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                None => return Ok(()),
            };
            let sort_masters = boolean_at(args, 2).unwrap_or(false);
            let silent = boolean_at(args, 3).unwrap_or(false);
            let Some(file) = file_impl_of(&file) else {
                return Ok(());
            };
            super::refused(file.add_masters_if_missing(&names, sort_masters, silent))?;
            Ok(())
        },
    );
    // `CanBeESL`, `CanBeLight` and `CanBeSmall` are one function upstream:
    // `File.LoadOrderFileID.IsLightSlot`.
    for name in ["CanBeESL", "CanBeLight", "CanBeSmall"] {
        add(adapter, name, 1, value_param(), |result, args| {
            *result = Value::Bool(false);
            if let Some(file) = file_at(args, 0) {
                *result = Value::Bool(file.get_load_order_file_id().is_light_slot());
            }
            Ok(())
        });
    }
    add(adapter, "CanBeMedium", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(file) = file_at(args, 0) {
            *result = Value::Bool(file.get_load_order_file_id().is_medium_slot());
        }
        Ok(())
    });
    add(adapter, "CleanMasters", 1, value_param(), |_result, args| {
        if let Some(file) = file_at(args, 0)
            && let Some(file) = file_impl_of(&file)
        {
            super::refused(file.clean_masters())?;
        }
        Ok(())
    });
    add(adapter, "SortMasters", 1, value_param(), |_result, args| {
        if let Some(file) = file_at(args, 0)
            && let Some(file) = file_impl_of(&file)
        {
            super::refused(file.sort_masters())?;
        }
        Ok(())
    });
    add(
        adapter,
        "FileFormIDtoLoadOrderFormID",
        2,
        params(&[VarType::Empty, VarType::Str]),
        |result, args| {
            *result = Value::Integer(0);
            if let Some(file) = file_at(args, 0) {
                let form_id = form_id_at(args, 1);
                if let Ok(converted) = file.file_form_id_to_load_order_form_id(form_id, true) {
                    *result = Value::Int64(i64::from(converted.to_cardinal()));
                }
            }
            Ok(())
        },
    );
    add(
        adapter,
        "LoadOrderFormIDtoFileFormID",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            *result = Value::Integer(0);
            if let Some(file) = file_at(args, 0) {
                let form_id = form_id_at(args, 1);
                // `TwbFile.LoadOrderFileIDtoFileFileID` raises for a FileID
                // that is neither the file nor one of its masters.
                let converted = super::refused(file.load_order_form_id_to_file_form_id(form_id, true))?;
                *result = Value::Int64(i64::from(converted.to_cardinal()));
            }
            Ok(())
        },
    );
    // `FileWriteToStream`: owed, the `TStream` value of the `Classes` unit
    // does not exist in the port yet. The file's bytes are written by the
    // `save` command.
    add(
        adapter,
        "FileWriteToStream",
        3,
        params(&[VarType::Empty, VarType::Empty, VarType::Empty]),
        |_result, args| {
            // `if Supports(IInterface(Args.Values[0]), IwbFile, _File) then
            // begin Stream := TStream(V2O(Args.Values[1])); if Assigned(
            // Stream) then ... _File.WriteToStream(Stream, rm)`: a nil or
            // non-file first argument and a nil stream are no-ops.
            let Some(file) = file_at(args, 0) else {
                return Ok(());
            };
            let Some(stream) = crate::script_rtl::classes::stream_of(args.values.get(1).unwrap_or(&Value::Empty))
            else {
                return Ok(());
            };
            // `case i of 0: rmNo; 2: rmSetInternal; else rmYes`.
            let reset = match integer_at(args, 2).unwrap_or(1) {
                0 => ResetModified::rmNo,
                2 => ResetModified::rmSetInternal,
                _ => ResetModified::rmYes,
            };
            let Some(file) = file_impl_of(&file) else {
                return Ok(());
            };
            let bytes = crate::save::write_file_bytes(&file, reset)
                .map_err(|message| ValueError::external_class("Exception", message))?;
            crate::script_rtl::classes::stream_write(&stream, &bytes)?;
            Ok(())
        },
    );
    add(adapter, "GetFileName", 1, value_param(), |result, args| {
        *result = Value::Str(Vec::new());
        // `if Supports(Args.Values[0], IwbFile, _File) then Value :=
        // _File.FileName else if Supports(Args.Values[0], IwbElement,
        // Element) then ... Element._File.FileName`.
        let name = if let Some(file) = file_at(args, 0) {
            file.get_name()
        } else {
            element_at(args, 0)
                .and_then(|element| element.get_file())
                .map(|file| file.get_name())
                .unwrap_or_default()
        };
        *result = Value::Str(name.into_bytes());
        Ok(())
    });
    // `GetIsESL`, `GetIsLight` and `GetIsSmall` are one function upstream:
    // `File.IsLight`.
    for name in ["GetIsESL", "GetIsLight", "GetIsSmall"] {
        add(adapter, name, 1, value_param(), |result, args| {
            *result = Value::Bool(false);
            if let Some(file) = file_at(args, 0)
                && let Some(file) = file_impl_of(&file)
            {
                *result = Value::Bool(file.get_is_light());
            }
            Ok(())
        });
    }
    add(adapter, "GetIsESM", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(file) = file_at(args, 0) {
            *result = Value::Bool(file.get_is_esm());
        }
        Ok(())
    });
    add(adapter, "GetIsMedium", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        if let Some(file) = file_at(args, 0)
            && let Some(file) = file_impl_of(&file)
        {
            *result = Value::Bool(file.get_is_medium());
        }
        Ok(())
    });
    // `SetIsESL`, `SetIsLight` and `SetIsSmall` are one function upstream:
    // `File.IsLight := Args.Values[1]`.
    for name in ["SetIsESL", "SetIsLight", "SetIsSmall"] {
        add(
            adapter,
            name,
            2,
            params(&[VarType::Empty, VarType::Bool]),
            |_result, args| {
                if let Some(file) = file_at(args, 0)
                    && let Some(file) = file_impl_of(&file)
                {
                    let value = boolean_at(args, 1)?;
                    super::refused(
                        file.set_module_flag(xedit_core::implementation::file_flags::ModuleFlag::Light, value),
                    )?;
                }
                Ok(())
            },
        );
    }
    add(
        adapter,
        "SetIsESM",
        2,
        params(&[VarType::Empty, VarType::Bool]),
        |_result, args| {
            if let Some(file) = file_at(args, 0)
                && let Some(file) = file_impl_of(&file)
            {
                let value = boolean_at(args, 1)?;
                super::refused(file.set_module_flag(xedit_core::implementation::file_flags::ModuleFlag::Esm, value))?;
            }
            Ok(())
        },
    );
    add(
        adapter,
        "SetIsMedium",
        2,
        params(&[VarType::Empty, VarType::Bool]),
        |_result, args| {
            if let Some(file) = file_at(args, 0)
                && let Some(file) = file_impl_of(&file)
            {
                let value = boolean_at(args, 1)?;
                super::refused(
                    file.set_module_flag(xedit_core::implementation::file_flags::ModuleFlag::Medium, value),
                )?;
            }
            Ok(())
        },
    );
    add(adapter, "GetLoadOrder", 1, value_param(), |result, args| {
        *result = Value::Integer(-1);
        if let Some(file) = file_at(args, 0) {
            *result = Value::Integer(file.get_load_order());
        }
        Ok(())
    });
    add(adapter, "GetLoadOrderFileID", 1, value_param(), |result, args| {
        *result = Value::Integer(-1);
        if let Some(file) = file_at(args, 0) {
            let id = file.get_load_order_file_id();
            *result = Value::Str(id.to_string().into_bytes());
        }
        Ok(())
    });
    add(
        adapter,
        "GetMasters",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |_result, args| {
            // `_File.GetMasters(TStrings(V2O(Args.Values[1])))`: the names of
            // the masters, in load order.
            let Some(file) = file_at(args, 0) else {
                return Ok(());
            };
            let names: Vec<Vec<u8>> = (0..file.get_master_count(true))
                .filter_map(|index| file.get_master(index, true))
                .map(|master| master.get_name().into_bytes())
                .collect();
            super::strings_add(args, 1, names);
            Ok(())
        },
    );
    add(adapter, "GetNewFormID", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        if let Some(file) = file_at(args, 0)
            && let Some(file) = file_impl_of(&file)
            && let Ok(form_id) = file.new_form_id()
        {
            *result = Value::Int64(i64::from(form_id.to_cardinal()));
        }
        Ok(())
    });
    add(
        adapter,
        "GroupBySignature",
        2,
        params(&[VarType::Empty, VarType::Str]),
        {
            let state = state.clone();
            move |result, args| {
                if let Some(file) = file_at(args, 0) {
                    let signature = super::signature_at(args, 1);
                    if let Some(group) = group_by_signature(&file, signature) {
                        *result = state.borrow().element_value(&group);
                    }
                }
                Ok(())
            }
        },
    );
    add(
        adapter,
        "HasGroup",
        2,
        params(&[VarType::Empty, VarType::Str]),
        |result, args| {
            *result = Value::Bool(false);
            if let Some(file) = file_at(args, 0) {
                let signature = super::signature_at(args, 1);
                *result = Value::Bool(group_by_signature(&file, signature).is_some());
            }
            Ok(())
        },
    );
    add(
        adapter,
        "HasMaster",
        2,
        params(&[VarType::Empty, VarType::Str]),
        |result, args| {
            *result = Value::Bool(false);
            if let Some(file) = file_at(args, 0)
                && let Some(file) = file_impl_of(&file)
            {
                *result = Value::Bool(file.has_master(&string_at(args, 1)));
            }
            Ok(())
        },
    );
    add(
        adapter,
        "MasterByIndex",
        2,
        params(&[VarType::Empty, VarType::Integer]),
        {
            let state = state.clone();
            move |result, args| {
                if let Some(file) = file_at(args, 0) {
                    let index = integer_at(args, 1)? as i32;
                    // The hardcoded file's records resolve into the game
                    // master, its single master (mirrors
                    // `ScriptRunState::master_at`, whichever of the two
                    // registrations the dispatch picks).
                    if file.get_file_states().contains(FileState::fsIsHardcoded) {
                        if index == 0
                            && let Some(master) = crate::script::game_master_element()
                        {
                            *result = state.borrow().element_value(&master);
                        }
                        return Ok(());
                    }
                    // `if Args.Values[1] < _File.MasterCount[True]`.
                    if index < file.get_master_count(true)
                        && let Some(master) = file.get_master(index, true)
                    {
                        *result = state.borrow().element_value(&(master as ElementRef));
                    }
                }
                Ok(())
            }
        },
    );
    add(adapter, "MasterCount", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        if let Some(file) = file_at(args, 0) {
            if file.get_file_states().contains(FileState::fsIsHardcoded) {
                // The game master, as `ScriptRunState::master_count`.
                *result = Value::Integer(i32::from(crate::script::game_master_element().is_some()));
            } else {
                *result = Value::Integer(file.get_master_count(true));
            }
        }
        Ok(())
    });
    add(
        adapter,
        "RecordByEditorID",
        2,
        params(&[VarType::Empty, VarType::Str]),
        {
            let state = state.clone();
            move |result, args| {
                if let Some(file) = file_at(args, 0) {
                    let editor_id = string_at(args, 1);
                    if let Some(record) = file.get_record_by_editor_id(&editor_id) {
                        *result = state.borrow().element_value(&(record as ElementRef));
                    }
                }
                Ok(())
            }
        },
    );
    add(
        adapter,
        "RecordByFormID",
        3,
        params(&[VarType::Empty, VarType::Integer, VarType::Bool]),
        {
            let state = state.clone();
            move |result, args| {
                if let Some(file) = file_at(args, 0) {
                    let form_id = form_id_at(args, 1);
                    let allow_injected = boolean_at(args, 2)?;
                    if let Ok(Some(record)) = file.get_record_by_form_id(form_id, true, allow_injected) {
                        *result = state.borrow().element_value(&(record as ElementRef));
                    }
                }
                Ok(())
            }
        },
    );
    add(
        adapter,
        "RecordByIndex",
        2,
        params(&[VarType::Empty, VarType::Integer]),
        {
            let state = state.clone();
            move |result, args| {
                if let Some(file) = file_at(args, 0) {
                    let index = integer_at(args, 1)? as i32;
                    // `if Args.Values[1] < _File.RecordCount then Value :=
                    // _File.Records[Args.Values[1]]`.
                    if index < file.get_record_count()
                        && let Some(implemented) = file.as_element_impl()
                        && let Some(file_impl) = implemented.file_impl()
                        && let Some(record) = usize::try_from(index)
                            .ok()
                            .and_then(|index| file_impl.records().get(index).cloned())
                    {
                        *result = state.borrow().element_value(&(record as ElementRef));
                    }
                }
                Ok(())
            }
        },
    );
    add(adapter, "RecordCount", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        if let Some(file) = file_at(args, 0) {
            *result = Value::Integer(file.get_record_count());
        }
        Ok(())
    });
}

/// Port of `TwbFile.GetGroupBySignature`: the top level group whose label is
/// the signature (`GroupType = 0`).
fn group_by_signature(file: &FileRef, signature: xedit_core::interface::Signature) -> Option<ElementRef> {
    let container = file.as_container()?;
    let label = u32::from_le_bytes(signature.0);
    (0..container.get_element_count())
        .filter_map(|index| container.get_element(index))
        .find(|element| {
            element.get_element_type() == xedit_core::interface::ElementType::etGroupRecord
                && element
                    .as_element_impl()
                    .and_then(|element| element.group_record_impl())
                    .is_some_and(|group| group.group_type() == 0 && group.group_label() == label)
        })
}

/// `TwbFormID.FromVar(Args.Values[i])`: the FormID of a script value.
fn form_id_at(args: &Args, index: usize) -> FormID {
    FormID::from_cardinal(integer_at(args, index).unwrap_or(0) as u32)
}
