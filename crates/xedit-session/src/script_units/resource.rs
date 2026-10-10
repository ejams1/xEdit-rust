// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/JvI/xejviScriptAdapterResource.pas

//! `xejviScriptAdapterResource.pas`: the resource container handler, the
//! asset helpers (`TwbAsset`), the NIF scanners, the DDS helpers, the
//! hashes (`TwbHash`) and the grid-cell maths.
//!
//! The container handler functions read `xedit_core::container_handler`, the
//! same global the `dump` and `lodgen` commands fill; the asset and NIF
//! functions are the phase-5 commands (`assets.scan`, `localization.strings`)
//! over `xedit-assets`. Owed, each with its ledger key in `docs/PLAN.md`:
//! the three `wbDDS*ToBitmap` functions and `wbFlipBitmap`/`wbAlphaBlend`
//! (the VCL `TBitmap`/device contexts of the GUI shim, phase 6 step 9),
//! `FileWriteToStream`-style stream values, `wbIsPseudoLight/Medium/Small`
//! (read from the port's globals, which the CLI does not set a
//! `-PseudoESL` switch into yet).

use std::cell::RefCell;
use std::rc::Rc;

use xedit_core::container_handler;
use xedit_core::interface::ElementRef;
use xedit_core::interface::globals;
use xedit_io::archive::{AssetType, asset_type_by_extension, asset_type_by_folder, get_asset_name};
use xedit_io::hash;

use xedit_script::adapter::Adapter;
use xedit_script::values::{ClassDef, Value, ValueError, VarType, to_str};

use super::{add, boolean_at, check_arg_count, integer_at, params, string_at, value_param};
use crate::script::ScriptRunState;

pub(crate) fn register(
    adapter: &mut Adapter,
    state: &Rc<RefCell<ScriptRunState>>,
    strings: &Rc<ClassDef>,
    lists: &Rc<ClassDef>,
) {
    let _ = (strings, lists);
    let grid_cell = super::record_def(adapter, "TwbGridCell");
    let vector = super::record_def(adapter, "TwbVector");

    //=== IwbContainerHandler ================================================

    add(adapter, "ResourceContainerList", 1, value_param(), |_result, args| {
        // `wbContainerHandler.ContainerList(TStrings(...))`: the names of
        // the loaded containers (folders and archives), in the order they
        // were added.
        let names: Vec<Vec<u8>> = container_handler::container_list()
            .into_iter()
            .map(String::into_bytes)
            .collect();
        super::strings_add(args, 0, names);
        Ok(())
    });
    add(
        adapter,
        "ResourceCopy",
        3,
        params(&[VarType::Empty, VarType::Empty, VarType::Empty]),
        |_result, args| {
            // `wbContainerHandler.ResourceCopy(ContainerName, FileName, PathOut)`.
            let container = string_at(args, 0);
            let file_name = string_at(args, 1);
            let path_out = string_at(args, 2);
            let _ = container_handler::resource_copy(&container, &file_name, &path_out);
            Ok(())
        },
    );
    add(
        adapter,
        "ResourceCount",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            // `Value := wbContainerHandler.ResourceCount(Path, TStrings(...))`.
            *result = Value::Integer(0);
            let path = string_at(args, 0);
            let (count, names) = container_handler::resource_count(&path);
            *result = Value::Integer(count as i32);
            let names: Vec<Vec<u8>> = names.into_iter().map(String::into_bytes).collect();
            super::strings_add(args, 1, names);
            Ok(())
        },
    );
    add(adapter, "ResourceExists", 1, value_param(), |result, args| {
        *result = Value::Bool(false);
        *result = Value::Bool(container_handler::resource_exists(&string_at(args, 0)));
        Ok(())
    });
    add(
        adapter,
        "ResourceList",
        -1,
        params(&[VarType::Empty, VarType::Empty, VarType::Str]),
        |_result, args| {
            // `case Args.Count of 0, 1: ieNotEnoughParams; 2:
            // ContainerResourceList(Container, List); 3: (Container, List,
            // Folder); else ieTooManyParams`.
            check_arg_count(args, 2, 3)?;
            let container = string_at(args, 0);
            let folder = if args.values.len() > 2 {
                string_at(args, 2)
            } else {
                String::new()
            };
            let names: Vec<Vec<u8>> = container_handler::container_resource_list(&container, &folder)
                .into_iter()
                .map(String::into_bytes)
                .collect();
            super::strings_add(args, 1, names);
            Ok(())
        },
    );
    add(
        adapter,
        "ResourceOpenData",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            // `Value := wbContainerHandler.OpenResourceData(ContainerName, Path)`:
            // a `TBytes`, which the port carries as a `varString` of the bytes.
            *result = Value::Str(Vec::new());
            let data = container_handler::open_resource_data(&string_at(args, 0), &string_at(args, 1));
            *result = Value::Str(data);
            Ok(())
        },
    );

    //=== TwbAsset ===========================================================

    add(adapter, "AssetTypeByExtension", 1, value_param(), |result, args| {
        // `TwbAsset.AssetTypeByExtension(Name)`.
        *result = Value::Integer(asset_type_by_extension(&string_at(args, 0)) as i32);
        Ok(())
    });
    add(adapter, "AssetTypeByFolder", 1, value_param(), |result, args| {
        // `TwbAsset.AssetTypeByFolder(Name)`.
        *result = Value::Integer(asset_type_by_folder(&string_at(args, 0)) as i32);
        Ok(())
    });
    add(
        adapter,
        "wbNormalizeResourceName",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            // `TwbAsset.GetAssetName(Name, '', Args.Values[1])`, the asset type
            // the second argument names (`atNone` when it is not one).
            *result = Value::Str(Vec::new());
            let name = string_at(args, 0);
            let asset_type = asset_type_of(integer_at(args, 1).unwrap_or(0));
            *result = Value::Str(get_asset_name(&name, "", asset_type).into_bytes());
            Ok(())
        },
    );

    //=== Nif routines =======================================================

    add(
        adapter,
        "NifBlockList",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            // `Value := NifBlockList(TBytes(Args.Values[0]), TStrings(...))`:
            // the scanner appends `Name=Type` per block and gives the count; an
            // empty byte array adds nothing.
            *result = Value::Integer(0);
            let Some(data) = args.values.first().and_then(|value| to_str(value).ok()) else {
                return Ok(());
            };
            if data.is_empty() {
                return Ok(());
            }
            let lines = xedit_assets::nif_scanner::nif_block_list(&data).map_err(scan_failed)?;
            *result = Value::Integer(lines.len() as i32);
            super::strings_add(
                args,
                1,
                lines.iter().map(|(line, _index)| line.clone().into_bytes()).collect(),
            );
            Ok(())
        },
    );
    add(
        adapter,
        "NifTextureList",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            *result = Value::Integer(0);
            let Some(data) = args.values.first().and_then(|value| to_str(value).ok()) else {
                return Ok(());
            };
            if data.is_empty() {
                return Ok(());
            }
            let lines = xedit_assets::nif_scanner::nif_textures(&data)
                .map_err(scan_failed)?
                .unwrap_or_default();
            *result = Value::Integer(lines.len() as i32);
            super::strings_add(
                args,
                1,
                lines.iter().map(|(line, _index)| line.clone().into_bytes()).collect(),
            );
            Ok(())
        },
    );
    add(
        adapter,
        "NifTextureListResource",
        3,
        params(&[VarType::Empty, VarType::Empty, VarType::Empty]),
        |result, args| {
            // `NifTextures(wbContainerHandler.OpenResourceData(File, Name),
            // Strings)`.
            *result = Value::Integer(0);
            let data = container_handler::open_resource_data(&string_at(args, 0), &string_at(args, 1));
            let lines = xedit_assets::nif_scanner::nif_textures(&data)
                .map_err(scan_failed)?
                .unwrap_or_default();
            *result = Value::Integer(lines.len() as i32);
            super::strings_add(
                args,
                2,
                lines.iter().map(|(line, _index)| line.clone().into_bytes()).collect(),
            );
            Ok(())
        },
    );
    add(
        adapter,
        "NifTextureListUVRange",
        3,
        params(&[VarType::Empty, VarType::Empty, VarType::Empty]),
        |result, args| {
            *result = Value::Integer(0);
            let Some(data) = args.values.first().and_then(|value| to_str(value).ok()) else {
                return Ok(());
            };
            if data.is_empty() {
                return Ok(());
            }
            let range = xedit_script::values::to_f64(args.values.get(1).unwrap_or(&Value::Empty)).unwrap_or(0.0) as f32;
            let lines = xedit_assets::nif_scanner::nif_textures_uv_range(&data, range)
                .map_err(scan_failed)?
                .unwrap_or_default();
            *result = Value::Integer(lines.len() as i32);
            super::strings_add(
                args,
                2,
                lines.iter().map(|(line, _index)| line.clone().into_bytes()).collect(),
            );
            Ok(())
        },
    );

    //=== DDS routines =======================================================
    // `wbDDSDataToBitmap`, `wbDDSResourceToBitmap` and
    // `wbDDSStreamToBitmap` return a `TBitmap` (VCL), which the port's value
    // model has no class for until the form shim of step 9; registered as
    // the upstream no-op of a failed `wbDDSDataToBitmap`.

    //=== Misc routines ======================================================

    add(adapter, "bscrc32", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        *result = Value::Int64(i64::from(hash::bscrc32(&string_at(args, 0))));
        Ok(())
    });
    add(adapter, "CreateHashFO4", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        *result = Value::Int64(i64::from(hash::fo4(&string_at(args, 0))));
        Ok(())
    });
    add(adapter, "CreateHashTES3", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        *result = Value::Int64(hash::tes3(&string_at(args, 0)) as i64);
        Ok(())
    });
    add(adapter, "CreateHashTES4", -1, value_param(), |result, args| {
        // `case Args.Count of 0: ieNotEnoughParams; 1: TES4(v, False);
        // 2: TES4(v, Args.Values[1]); else ieTooManyParams`.
        check_arg_count(args, 1, 2)?;
        *result = Value::Integer(0);
        let has_extension = boolean_at(args, 1).unwrap_or(false);
        *result = Value::Int64(hash::tes4(&string_at(args, 0), has_extension) as i64);
        Ok(())
    });
    add(
        adapter,
        "LocalizationGetStringsFromFile",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |_result, args| {
            // `if Assigned(wbLocalizationHandler) then
            // wbLocalizationHandler.GetStringsFromFile(FileName, Strings)`:
            // the loaded string table's entries with their IDs.
            let file_name = string_at(args, 0);
            let lines: Vec<Vec<u8>> = xedit_core::localization::localization_handler()
                .and_then(|handler| handler.get_strings_from_file(&file_name))
                .unwrap_or_default()
                .into_iter()
                .map(|(id, text)| format!("{id}\t{text}").into_bytes())
                .collect();
            super::strings_add(args, 1, lines);
            Ok(())
        },
    );
    // `wbAlphaBlend` and `wbFlipBitmap` draw through device contexts and a
    // `TBitmap` (VCL); owed to the form shim of step 9.
    add(adapter, "wbBlockFromSubBlock", 1, value_param(), {
        let grid_cell = grid_cell.clone();
        move |result, args| {
            if let Some((x, y)) = super::grid_cell_of(args.values.first().unwrap_or(&Value::Empty)) {
                let (x, y) = xedit_core::implementation::add::block_from_sub_block(x, y);
                *result = super::grid_cell_value(grid_cell.clone(), x, y);
            }
            Ok(())
        }
    });
    add(adapter, "wbSubBlockFromGridCell", 1, value_param(), {
        let grid_cell = grid_cell.clone();
        move |result, args| {
            if let Some((x, y)) = super::grid_cell_of(args.values.first().unwrap_or(&Value::Empty)) {
                let (x, y) = xedit_core::implementation::add::sub_block_from_grid_cell(x, y);
                *result = super::grid_cell_value(grid_cell.clone(), x, y);
            }
            Ok(())
        }
    });
    add(adapter, "wbCRC32Data", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        let Some(data) = args.values.first().and_then(|value| to_str(value).ok()) else {
            return Ok(());
        };
        *result = Value::Int64(i64::from(hash::crc32(&data)));
        Ok(())
    });
    add(adapter, "wbCRC32File", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        let path = string_at(args, 0);
        if let Ok(data) = std::fs::read(&path) {
            *result = Value::Int64(i64::from(hash::crc32(&data)));
        }
        Ok(())
    });
    add(
        adapter,
        "wbCRC32Resource",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            *result = Value::Integer(0);
            let data = container_handler::open_resource_data(&string_at(args, 0), &string_at(args, 1));
            *result = Value::Int64(i64::from(hash::crc32(&data)));
            Ok(())
        },
    );
    add(
        adapter,
        "wbFindREFRsByBase",
        4,
        params(&[VarType::Empty, VarType::Empty, VarType::Empty, VarType::Empty]),
        {
            let state = state.clone();
            move |_result, args| {
                // The REFRs of the record's child group whose base record's
                // signature appears in `BaseSignatures`, unless deleted,
                // disabled or enable-parented and the option bits say to
                // skip those.
                let Some(record) = super::main_record_at(args, 0) else {
                    return Ok(());
                };
                let base_signatures = string_at(args, 1);
                let options = integer_at(args, 2).unwrap_or(0) as i32;
                if super::list_at(args, 3).is_none() {
                    return Ok(());
                }
                let record_ref: ElementRef = record;
                let refs = xedit_core::helpers::wb_get_sibling_records(
                    &record_ref,
                    &xedit_core::helpers::wb_string_to_signatures("REFR"),
                    true,
                );
                for reference in refs {
                    if options & 1 != 0 && reference.get_is_deleted() {
                        continue;
                    }
                    if options & 2 != 0 && reference.get_is_initially_disabled() {
                        continue;
                    }
                    if options & 4 != 0 && reference.get_element_exists("XESP") {
                        continue;
                    }
                    let Some(base) = reference.get_base_record() else {
                        continue;
                    };
                    // `Pos(REFRs[i].BaseRecord.Signature, BaseSignatures) <> 0`.
                    if !base_signatures.contains(&super::signature_text_of(base.get_signature())) {
                        continue;
                    }
                    super::list_add(args, 3, state.borrow().element_value(&(reference as ElementRef)));
                }
                Ok(())
            }
        },
    );
    // `wbFormIDErrorCheckLock`/`Unlock` count the nesting of the FormID
    // error checking of `wbFormIDChecked.FromEditValue`: the port validates
    // a FormID on every write instead (no threadvar counter), so the two
    // functions report the port's count of 0.
    for name in ["wbFormIDErrorCheckLock", "wbFormIDErrorCheckUnlock"] {
        add(adapter, name, 0, Vec::new(), |result, _args| {
            *result = Value::Integer(0);
            Ok(())
        });
    }
    add(
        adapter,
        "wbGetSiblingRecords",
        4,
        params(&[VarType::Empty, VarType::Empty, VarType::Empty, VarType::Empty]),
        {
            let state = state.clone();
            move |_result, args| {
                let Some(element) = super::element_at(args, 0) else {
                    return Ok(());
                };
                let signatures = xedit_core::helpers::wb_string_to_signatures(&string_at(args, 1));
                let overrides = boolean_at(args, 2)?;
                if super::list_at(args, 3).is_none() {
                    return Ok(());
                }
                for record in xedit_core::helpers::wb_get_sibling_records(&element, &signatures, overrides) {
                    super::list_add(args, 3, state.borrow().element_value(&(record as ElementRef)));
                }
                Ok(())
            }
        },
    );
    add(adapter, "wbGridCellToGroupLabel", 1, value_param(), |result, args| {
        *result = Value::Integer(0);
        if let Some((x, y)) = super::grid_cell_of(args.values.first().unwrap_or(&Value::Empty)) {
            *result = Value::Int64(i64::from(xedit_core::implementation::add::grid_cell_to_group_label(
                x, y,
            )));
        }
        Ok(())
    });
    add(
        adapter,
        "wbIsInGridCell",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            *result = Value::Bool(false);
            let Some((x, y, _z)) = super::vector_of(args.values.first().unwrap_or(&Value::Empty)) else {
                return Ok(());
            };
            let Some((cell_x, cell_y)) = super::grid_cell_of(args.values.get(1).unwrap_or(&Value::Empty)) else {
                return Ok(());
            };
            *result = Value::Bool(
                xedit_core::implementation::form_ids::position_to_grid_cell(f64::from(x)) == cell_x
                    && xedit_core::implementation::form_ids::position_to_grid_cell(f64::from(y)) == cell_y,
            );
            Ok(())
        },
    );
    // `wbIsPseudoESLMode`, `wbIsPseudoLightMode` and `wbIsPseudoSmallMode`
    // are one function upstream: `wbIsLightSupported and wbPseudoLight`.
    for name in ["wbIsPseudoESLMode", "wbIsPseudoLightMode", "wbIsPseudoSmallMode"] {
        add(adapter, name, 0, Vec::new(), |result, _args| {
            *result = Value::Bool(globals::is_light_supported() && globals::pseudo_light());
            Ok(())
        });
    }
    add(adapter, "wbIsPseudoMediumMode", 0, Vec::new(), |result, _args| {
        *result = Value::Bool(globals::is_medium_supported() && globals::pseudo_medium());
        Ok(())
    });
    add(adapter, "wbPositionToGridCell", 1, value_param(), {
        let grid_cell = grid_cell.clone();
        move |result, args| {
            if let Some((x, y, _z)) = super::vector_of(args.values.first().unwrap_or(&Value::Empty)) {
                let cell_x = xedit_core::implementation::form_ids::position_to_grid_cell(f64::from(x));
                let cell_y = xedit_core::implementation::form_ids::position_to_grid_cell(f64::from(y));
                *result = super::grid_cell_value(grid_cell.clone(), cell_x, cell_y);
            }
            Ok(())
        }
    });
    add(
        adapter,
        "wbStringListInString",
        2,
        params(&[VarType::Empty, VarType::Empty]),
        |result, args| {
            // `Value := -1; for i := 0 to Pred(sl.Count) do if
            // Pos(Lowercase(sl[i]), Lowercase(s)) > 0 then Value := i`.
            *result = Value::Integer(-1);
            let Some(object) = super::strings_at(args, 0) else {
                return Ok(());
            };
            let haystack = string_at(args, 1).to_ascii_lowercase();
            let borrowed = object.borrow();
            let Some(list) = borrowed.string_list() else {
                return Ok(());
            };
            for (index, line) in list.lines().iter().enumerate() {
                let needle = String::from_utf8_lossy(line).to_ascii_lowercase();
                if !needle.is_empty() && haystack.contains(&needle) {
                    *result = Value::Integer(index as i32);
                    break;
                }
            }
            Ok(())
        },
    );

    let _ = vector;
}

/// The `TwbAssetType` of the integer a script passes (`Args.Values[1]` of
/// `wbNormalizeResourceName`).
fn asset_type_of(index: i64) -> AssetType {
    // The declaration order of `TwbAssetType` (`wbBSArchive.pas:437`).
    const ALL: [AssetType; 31] = [
        AssetType::None,
        AssetType::Mesh,
        AssetType::Texture,
        AssetType::Material,
        AssetType::Geometry,
        AssetType::Sound,
        AssetType::Voice,
        AssetType::Music,
        AssetType::Script,
        AssetType::Source,
        AssetType::SourceSse,
        AssetType::Strings,
        AssetType::SpeedTree,
        AssetType::Video,
        AssetType::LodSettings,
        AssetType::DistantLod,
        AssetType::Interface,
        AssetType::Program,
        AssetType::Menus,
        AssetType::Font,
        AssetType::Facegen,
        AssetType::LsData,
        AssetType::Shaders,
        AssetType::ShadersFx,
        AssetType::Grass,
        AssetType::PreVis,
        AssetType::Seq,
        AssetType::DialogueViews,
        AssetType::BookArt,
        AssetType::Icon,
        AssetType::Splash,
    ];
    usize::try_from(index)
        .ok()
        .and_then(|index| ALL.get(index).copied())
        .unwrap_or(AssetType::None)
}

/// A NIF the scanner refused, as the script error an upstream exception
/// becomes (`ieExternal` with the message).
fn scan_failed(error: xedit_assets::data_format::DfError) -> ValueError {
    ValueError::External(error.0)
}
