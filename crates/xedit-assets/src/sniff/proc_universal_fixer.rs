// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcUniversalFixer.pas

//! `Universal fixer`: a list of fixes for common errors of meshes (string
//! indices, link arrays, redundant blocks, absolute asset paths, shader
//! types and flags, `BSXFlags`, hard-coded names, animation targets,
//! collision settings, particle data, consistency flags), with an optional
//! log file of what changed.
//!
//! `FixEditorMarker` and `FixObjectPalette` are not called upstream; the
//! first is ported for the record, the second is commented out upstream.

use std::cmp::Ordering;

use crate::data_format::{DfError, El, R, Tree};
use crate::data_format_nif::{
    NifFile, NifOptions, NifVersion, block, block_add_extra_data, block_by_name, block_by_type,
    block_extra_datas_by_type, block_get_controller, block_get_skin, block_get_string_palette_string,
    block_is_editor_marker, block_is_hidden, block_is_ni_object, block_property_by_type, block_referenced_by,
    block_remove_branch, block_strings, block_type, blocks_by_type, blocks_count, detect_bsx_flags, get_assets,
    get_link_arrays, get_unique_name, header, nifblk, root_node,
};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, StopContext, Storage, access_violation, ansi_same_text,
    extract_file_path, same_value, same_value_single, string_list_file_bytes, string_list_text_of,
};
use crate::variant::Variant;

pub struct ProcUniversalFixer {
    base: ProcBase,
    /// `chkSaveLog`.
    save_log: bool,
    /// `chkOpenLog`: the GUI opens the log; the port does not.
    open_log: bool,
    /// `edLog`.
    log_file_name: String,
}

impl ProcUniversalFixer {
    pub fn new() -> ProcUniversalFixer {
        ProcUniversalFixer {
            base: ProcBase::new("Universal fixer", GameType::ALL, &["nif"]),
            save_log: false,
            open_log: false,
            log_file_name: String::new(),
        }
    }
}

/// `Log.Add`.
type Log = Vec<String>;

fn el(tree: &mut Tree, element: El, path: &str) -> R<El> {
    tree.elements(element, path)?.ok_or_else(access_violation)
}

/// `Elements[aPath].LinksTo`, which reads through nil when the element is
/// missing.
fn link(tree: &mut Tree, element: El, path: &str) -> R<Option<El>> {
    let element = el(tree, element, path)?;
    tree.links_to(element)
}

fn flag(tree: &mut Tree, element: El, path: &str) -> R<bool> {
    tree.native_values(element, path)?.to_bool()
}

fn set_flag(tree: &mut Tree, element: El, path: &str, value: bool) -> R<()> {
    tree.set_native_values(element, path, Variant::Bool(value))
}

fn name(tree: &mut Tree, element: El) -> R<String> {
    tree.name(element)
}

/// Variant `<>` of two values.
fn variants_differ(a: &Variant, b: &Variant) -> R<bool> {
    Ok(a.compare(b)? != Ordering::Equal)
}

// ===========================================================================
/// `FixStringIndices`.
fn fix_string_indices(tree: &mut Tree, log: &mut Log) -> R<bool> {
    let mut result = false;
    if tree.nif.nif_version < NifVersion::Fo3 {
        return Ok(false);
    }
    let header = header(tree)?;
    let num_strings = el(tree, header, "Num Strings")?;
    let n = tree.native_value(num_strings)?.to_i64()?;
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        for string in block_strings(tree, b) {
            let value = tree.native_value(string)?.to_i64()?;
            if value >= n {
                let path = tree.path(string)?;
                log.push(format!("\t{path}: Invalid string index {value} set to -1 (None)"));
                tree.set_native_value(string, Variant::Int(-1))?;
                result = true;
            }
        }
    }
    Ok(result)
}

// ===========================================================================
/// `FixArrayLinks`.
fn fix_array_links(tree: &mut Tree, log: &mut Log) -> R<bool> {
    let mut result = false;
    for links in get_link_arrays(tree)? {
        // Skip the arrays of a fixed length.
        if tree.def(links)?.size > 0 {
            continue;
        }
        let mut i = tree.count(links) - 1;
        while i >= 0 {
            let item = tree.item(links, i)?;
            let n = tree.native_value(item)?.to_i64()?;
            if n < 0 {
                let path = tree.path(item)?;
                log.push(format!("\t{path}: Removed a null link"));
                tree.delete(links, i)?;
                result = true;
            } else if n >= i64::from(blocks_count(tree)?) {
                let path = tree.path(item)?;
                log.push(format!("\t{path}: Removed a broken link"));
                tree.delete(links, i)?;
                result = true;
            } else {
                for j in 0..i {
                    let other = tree.item(links, j)?;
                    if tree.native_value(other)?.to_i64()? == n {
                        let path = tree.path(item)?;
                        log.push(format!("\t{path}: Removed a repeated link"));
                        tree.delete(links, i)?;
                        result = true;
                        break;
                    }
                }
            }
            i -= 1;
        }
    }
    Ok(result)
}

/// `TPath.IsPathRooted`: a path delimiter first, or a drive.
fn is_path_rooted(path: &str) -> bool {
    let mut chars = path.chars();
    match (chars.next(), chars.next()) {
        (Some('\\' | '/'), _) => true,
        (Some(letter), Some(':')) => letter.is_ascii_alphabetic(),
        _ => false,
    }
}

/// `string.StartsWith(Value, True)` and `EndsWith(Value, True)`.
fn starts_with_text(text: &str, prefix: &str) -> bool {
    text.to_lowercase().starts_with(&prefix.to_lowercase())
}

fn ends_with_text(text: &str, suffix: &str) -> bool {
    text.to_lowercase().ends_with(&suffix.to_lowercase())
}

// ===========================================================================
/// `FixAbsolutePaths`.
fn fix_absolute_paths(tree: &mut Tree, log: &mut Log) -> R<bool> {
    const SUB_FOLDERS: [&str; 9] = [
        "\\meshes\\",
        "\\textures\\",
        "\\materials\\",
        "/meshes/",
        "/textures/",
        "/materials/",
        "\\data\\",
        "/data/",
        "\\data files\\",
    ];
    let mut result = false;
    for element in get_assets(tree)? {
        let asset_name = tree.edit_value(element)?;
        let mut new_name = asset_name.clone();

        // Bethesda's slop.
        if new_name.contains("\u{8}NOR") {
            new_name.clear();
        }

        // Remove the control characters.
        new_name.retain(|c| c >= ' ');

        // The delimiter: a path with both is changed to backslashes.
        let delim = if new_name.contains('\\') && new_name.contains('/') {
            new_name = new_name.replace('/', "\\");
            "\\"
        } else if new_name.contains('/') {
            "/"
        } else {
            "\\"
        };

        // Double delimiters.
        new_name = new_name.replace(&format!("{delim}{delim}"), delim);

        if is_path_rooted(&new_name) {
            // Remove the path up to the folder of the asset.
            let lower = new_name.to_ascii_lowercase();
            let fixed = SUB_FOLDERS.iter().find_map(|sub_folder| lower.find(sub_folder));
            match fixed {
                Some(p) => new_name = new_name[p + 1..].to_owned(),
                None => {
                    // Can not fix it: a warning.
                    let path = tree.path(element)?;
                    log.push(format!("\t{path}: Couldn't fix absolute path \"{asset_name}\""));
                    continue;
                }
            }
        } else if tree.nif.nif_version > NifVersion::Tes3
            && ends_with_text(&new_name, ".dds")
            && !(starts_with_text(&new_name, &format!("textures{delim}"))
                || starts_with_text(&new_name, &format!("data{delim}textures{delim}")))
        {
            // Textures start with the textures folder or data\textures.
            new_name = format!("textures{delim}{new_name}");
        }

        if asset_name != new_name {
            let path = tree.path(element)?;
            log.push(format!(
                "\t{path}: Path changed from \"{asset_name}\" to \"{new_name}\""
            ));
            tree.set_edit_value(element, &new_name)?;
            result = true;
        }
    }
    Ok(result)
}

// ===========================================================================
/// `FixEditorMarker`: not called upstream.
#[allow(dead_code)]
fn fix_editor_marker(tree: &mut Tree, log: &mut Log) -> R<bool> {
    let mut result = false;
    let version = tree.nif.nif_version;
    if version < NifVersion::Fo3 {
        return Ok(false);
    }
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        if block_is_editor_marker(tree, b)? && tree.elements(b, "Flags")?.is_some() {
            let hidden = block_is_hidden(tree, b)?;
            let flags = tree.native_values(b, "Flags")?.to_i64()?;
            if matches!(version, NifVersion::Tes5 | NifVersion::Sse) && !hidden {
                tree.set_native_values(b, "Flags", Variant::Int(flags | 1))?;
                log.push(format!("\t{}: Added Hidden flag to EditorMarker", name(tree, b)?));
                result = true;
            } else if version == NifVersion::Fo3 && hidden {
                tree.set_native_values(b, "Flags", Variant::Int(flags & !1))?;
                log.push(format!("\t{}: Removed Hidden flag from EditorMarker", name(tree, b)?));
                result = true;
            }
        }
    }
    Ok(result)
}

// ===========================================================================
/// `FixBSXFlags`.
fn fix_bsx_flags(tree: &mut Tree, log: &mut Log) -> R<bool> {
    let mut result = false;
    if tree.nif.nif_version < NifVersion::Fo3 {
        return Ok(false);
    }
    if blocks_count(tree)? == 0 {
        return Ok(false);
    }
    let mut flags = i64::from(detect_bsx_flags(tree)?);

    let bsx = match block_by_type(tree, "BSXFlags", false)? {
        None => {
            // Missing and not needed: nothing to do.
            if flags == 0 {
                return Ok(false);
            }
            let root = root_node(tree)?;
            let bsx = block_add_extra_data(tree, root, "BSXFlags")?;
            tree.set_edit_values(bsx, "Name", "BSX")?;
            log.push(format!("\t{}: Added missing BSXFlags", name(tree, bsx)?));
            result = true;
            bsx
        }
        Some(bsx) => {
            if flags == 0 {
                log.push(format!("\t{}: Removed emtpy BSXFlags", name(tree, bsx)?));
                block_remove_branch(tree, bsx, true)?;
                // UPSTREAM-QUIRK: the code below reads the freed block, which
                // has no elements left: its flags read as 0 and the check
                // finds nothing to change.
                return Ok(true);
            }
            bsx
        }
    };

    let old_flags = tree.native_values(bsx, "Flags")?.to_i64()?;
    // Fallout 4 meshes keep their Complex and Dynamic flags, which can not
    // be detected.
    if tree.nif.nif_version >= NifVersion::Fo4 {
        if old_flags & (1 << 3) != 0 {
            flags |= 1 << 3;
        }
        if old_flags & (1 << 6) != 0 {
            flags |= 1 << 6;
        }
    }
    // Articulated changes grabbing only and is unknown when to set: kept.
    if old_flags & (1 << 7) != 0 {
        flags |= 1 << 7;
    }

    if old_flags != flags {
        tree.set_native_values(bsx, "Flags", Variant::Int(flags))?;
        let value = tree.edit_values(bsx, "Flags")?;
        log.push(format!("\t{}: Flags set to \"{value}\"", name(tree, bsx)?));
        result = true;
    }
    Ok(result)
}

// ===========================================================================
/// `FixHardcodedNames`.
fn fix_hardcoded_names(tree: &mut Tree, log: &mut Log) -> R<bool> {
    const NAMES: [(&str, &str); 15] = [
        ("BSBehaviorGraphExtraData", "BGED"),
        ("BSBoneLODExtraData", "BSBoneLOD"),
        ("BSBound", "BBX"),
        ("BSClothExtraData", "CED"),
        ("BSConnectPoint::Children", "CPT"),
        ("BSConnectPoint::Parents", "CPA"),
        ("BSDecalPlacementVectorExtraData", "DVPG"),
        ("BSDistantObjectLargeRefExtraData", "DOLRED"),
        ("BSEyeCenterExtraData", "ECED"),
        ("BSFurnitureMarker", "FRN"),
        ("BSFurnitureMarkerNode", "FRN"),
        ("BSInvMarker", "INV"),
        ("BSPositionData", "BSPosData"),
        ("BSWArray", "BSW"),
        ("BSXFlags", "BSX"),
    ];
    let mut result = false;
    for i in 0..blocks_count(tree)? {
        let b = block(tree, i)?;
        if tree.elements(b, "Name")?.is_none() {
            continue;
        }
        let current = tree.edit_values(b, "Name")?;
        if let Some((_, hard_name)) = NAMES.iter().find(|(kind, _)| block_type(tree, b) == *kind)
            && current != *hard_name
        {
            tree.set_edit_values(b, "Name", hard_name)?;
            log.push(format!(
                "\t{}: Renamed from \"{current}\" to \"{hard_name}\"",
                name(tree, b)?
            ));
            result = true;
        }

        if tree.nif.nif_version == NifVersion::Tes4 && block_is_ni_object(tree, b, "NiTriBasedGeom", true) {
            let material = block_property_by_type(tree, b, "NiMaterialProperty", false)?;
            // Only rendered shapes (with NiTexturingProperty) need a named
            // material.
            if let Some(material) = material
                && tree.edit_values(material, "Name")?.is_empty()
                && block_property_by_type(tree, b, "NiTexturingProperty", false)?.is_some()
            {
                let new_name = get_unique_name(tree, "Material")?;
                tree.set_edit_values(material, "Name", &new_name)?;
                log.push(format!("\t{}: Renamed to \"{new_name}\"", name(tree, material)?));
                result = true;
            }
        }
    }
    Ok(result)
}

// ===========================================================================
/// `SortByUserData`.
fn sort_by_user_data(tree: &mut Tree, a: El, b: El) -> i32 {
    let (a, b) = (tree.user_data(a), tree.user_data(b));
    match a.cmp(&b) {
        Ordering::Less => -1,
        Ordering::Greater => 1,
        Ordering::Equal => 0,
    }
}

/// `GetControlledBlockTarget` of `FixAnim`.
fn get_controlled_block_target(tree: &mut Tree, b: El) -> R<Option<El>> {
    if tree.nif.nif_version >= NifVersion::Fo3 {
        let node_name = tree.edit_values(b, "Node Name")?;
        if !node_name.is_empty() {
            return block_by_name(tree, &node_name, "");
        }
    } else if let Some(palette) = tree.elements(b, "String Palette")? {
        // Oblivion meshes keep the node name in the string palette.
        if let Some(palette) = tree.links_to(palette)? {
            let offset = tree.native_values(b, "Node Name Offset")?.to_i32()?;
            let node_name = block_get_string_palette_string(tree, palette, offset)?;
            if !node_name.is_empty() {
                return block_by_name(tree, &node_name, "");
            }
        }
    }
    Ok(None)
}

/// `TStringList.IndexOf` of an unsorted list: ignores case.
fn index_of(list: &[(String, Option<El>)], text: &str) -> Option<usize> {
    list.iter().position(|(item, _)| ansi_same_text(item, text))
}

/// `TStringList.Text`.
fn text_of(list: &[(String, Option<El>)]) -> String {
    let lines: Vec<String> = list.iter().map(|(item, _)| item.clone()).collect();
    string_list_text_of(&lines)
}

/// `FixAnim`.
fn fix_anim(tree: &mut Tree, log: &mut Log) -> R<bool> {
    let mut result = false;

    // The target of the controllers of the shapes.
    for b in blocks_by_type(tree, "NiObjectNET", true)? {
        let mut controller = link(tree, b, "Controller")?;
        while let Some(current) = controller {
            if !block_is_ni_object(tree, current, "NiTimeController", true) {
                break;
            }
            if link(tree, current, "Target")?.is_none() {
                let index = tree.index(b)?;
                tree.set_native_values(current, "Target", Variant::Int(i64::from(index)))?;
                log.push(format!(
                    "\t{}: Empty Target set to {}",
                    name(tree, current)?,
                    name(tree, b)?
                ));
                result = true;
            }
            // Down the chain of Next Controller.
            controller = link(tree, current, "Next Controller")?;
        }
    }

    // The controller sequences.
    let Some(manager) = block_by_type(tree, "NiControllerManager", false)? else {
        return Ok(result);
    };
    let Some(seqs) = tree.elements(manager, "Controller Sequences")? else {
        return Ok(result);
    };

    // The old and new lists of the extra targets.
    let mut sl_new: Vec<(String, Option<El>)> = Vec::new();
    let mut sl_old: Vec<(String, Option<El>)> = Vec::new();

    for i in 0..tree.count(seqs) {
        let item = tree.item(seqs, i)?;
        let Some(seq) = tree.links_to(item)? else {
            continue;
        };

        let target_element = link(tree, manager, "Target")?.ok_or_else(access_violation)?;
        let target_name = tree.native_values(target_element, "Name")?;
        let accum = tree.native_values(seq, "Accum Root Name")?;
        if variants_differ(&accum, &target_name)? {
            tree.set_native_values(seq, "Accum Root Name", target_name)?;
            log.push(format!(
                "\t{}: Set Accum Root to {}",
                name(tree, seq)?,
                name(tree, target_element)?
            ));
            result = true;
        }

        // The controlled blocks, in reverse: some are deleted.
        let blocks = el(tree, seq, "Controlled Blocks")?;
        let mut j = tree.count(blocks) - 1;
        while j >= 0 {
            let entry = tree.item(blocks, j)?;
            let target = get_controlled_block_target(tree, entry)?;

            // The index of the target node, for the sort.
            if let Some(target) = target
                && block_is_ni_object(tree, target, "NiAVObject", true)
            {
                let index = tree.index(target)?;
                tree.set_user_data(entry, index);
                j -= 1;
                continue;
            }

            // The target is missing or not a node: the block goes, with
            // its interpolator and controller.
            if let Some(interpolator) = link(tree, entry, "Interpolator")? {
                tree.set_native_values(entry, "Interpolator", Variant::Int(-1))?;
                block_remove_branch(tree, interpolator, false)?;
            }
            let entry = tree.item(blocks, j)?;
            if let Some(controller) = link(tree, entry, "Controller")? {
                tree.set_native_values(entry, "Controller", Variant::Int(-1))?;
                block_remove_branch(tree, controller, false)?;
            }

            log.push(format!(
                "\t{}: Removed Controlled block #{j} because Target is missing or not a visible NiAVObject",
                name(tree, seq)?
            ));
            tree.delete(blocks, j)?;
            result = true;
            j -= 1;
        }

        let count = tree.count(blocks);
        tree.set_native_values(seq, "Num Controlled Blocks", Variant::Int(i64::from(count)))?;

        // Sort the controlled blocks by the index of their target.
        if tree.sort(blocks, &mut sort_by_user_data)? {
            log.push(format!(
                "\t{}: Sorted Controlled Blocks by Target node index",
                name(tree, seq)?
            ));
            result = true;
        }

        // The target nodes after the deletions, for the extra targets.
        for j in 0..tree.count(blocks) {
            let entry = tree.item(blocks, j)?;
            let target = get_controlled_block_target(tree, entry)?.ok_or_else(access_violation)?;
            let key = format!("{}{}", name(tree, target)?, tree.edit_values(target, "Name")?);
            if index_of(&sl_new, &key).is_none() {
                sl_new.push((key, Some(target)));
            }
        }
    }

    // The object palette.
    if let Some(palette) = block_by_type(tree, "NiDefaultAVObjectPalette", false)? {
        let objs = el(tree, palette, "Objects")?;
        for i in 0..tree.count(objs) {
            let item = tree.item(objs, i)?;
            if let Some(obj) = link(tree, item, "AV Object")? {
                let key = format!("{}{}", name(tree, obj)?, tree.edit_values(obj, "Name")?);
                sl_old.push((key, Some(obj)));
            }
        }
        // Update when different.
        if tree.count(objs) != sl_new.len() as i32 || text_of(&sl_old) != text_of(&sl_new) {
            tree.set_count(objs, sl_new.len() as i32)?;
            for (i, (_, obj)) in sl_new.iter().enumerate() {
                let obj = obj.ok_or_else(access_violation)?;
                let item = tree.item(objs, i as i32)?;
                let obj_name = tree.edit_values(obj, "Name")?;
                tree.set_edit_values(item, "Name", &obj_name)?;
                let index = tree.index(obj)?;
                tree.set_native_values(item, "AV Object", Variant::Int(i64::from(index)))?;
            }
            log.push(format!("\t{}: Updated AV Objects", name(tree, palette)?));
            result = true;
        }
    }

    // The extra targets.
    let Some(multi_target) = link(tree, manager, "Next Controller")? else {
        return Ok(result);
    };
    let Some(extra_targets) = tree.elements(multi_target, "Extra Targets")? else {
        return Ok(result);
    };

    sl_old.clear();
    for i in 0..tree.count(extra_targets) {
        let item = tree.item(extra_targets, i)?;
        if let Some(target) = tree.links_to(item)? {
            let key = format!("{}{}", name(tree, target)?, tree.edit_values(target, "Name")?);
            sl_old.push((key, None));
        }
    }

    if tree.count(extra_targets) != sl_new.len() as i32 || text_of(&sl_old) != text_of(&sl_new) {
        tree.set_count(extra_targets, sl_new.len() as i32)?;
        for (i, (_, target)) in sl_new.iter().enumerate() {
            let target = target.ok_or_else(access_violation)?;
            let item = tree.item(extra_targets, i as i32)?;
            let index = tree.index(target)?;
            tree.set_native_value(item, Variant::Int(i64::from(index)))?;
            let value = tree.native_value(item)?.to_i32()?;
            tree.set_user_data(item, value);
        }
        tree.sort(extra_targets, &mut sort_by_user_data)?;
        log.push(format!("\t{}: Updated Extra Targets", name(tree, multi_target)?));
        result = true;
    }
    Ok(result)
}

// ===========================================================================
/// `UpdateElement` of `FixCollision`.
fn update_element(tree: &mut Tree, log: &mut Log, b: El, path: &str, value: &str, ok_values: &str) -> R<bool> {
    let Some(element) = tree.elements(b, path)? else {
        return Ok(false);
    };
    let ok_values = if ok_values.is_empty() {
        value.to_owned()
    } else {
        format!("{ok_values},{value}")
    };
    let current = tree.edit_value(element)?;
    // UPSTREAM-QUIRK: `Pos` of an empty value is 0, so an empty value is
    // changed.
    if current.is_empty() || !ok_values.contains(&current) {
        log.push(format!(
            "\t{}: {path} changed from {current} to {value}",
            name(tree, b)?
        ));
        tree.set_edit_value(element, value)?;
        return Ok(true);
    }
    Ok(false)
}

/// `BadTensor`.
fn bad_tensor(t: f32) -> bool {
    t.is_nan() || same_value_single(t, 0.0) || t < 0.0
}

fn native_f64(tree: &mut Tree, b: El, path: &str) -> R<f64> {
    tree.native_values(b, path)?.to_f64()
}

/// The settings of a static or animated static rigid body of
/// `FixCollision`.
fn fix_fixed_body(tree: &mut Tree, log: &mut Log, rigid: El, s: &str, motion_system: &str) -> R<bool> {
    let mut result = false;
    result = update_element(tree, log, rigid, "Motion System", motion_system, "")? || result;
    result = update_element(tree, log, rigid, "Motion Quality", "MO_QUAL_FIXED", "")? || result;
    result = update_element(tree, log, rigid, "Deactivator Type", "DEACTIVATOR_NEVER", "")? || result;
    result = update_element(tree, log, rigid, "Enable Deactivation", "no", "")? || result;
    result = update_element(tree, log, rigid, "Solver Deactivation", "SOLVER_DEACTIVATION_OFF", "")? || result;
    if !same_value(native_f64(tree, rigid, "Mass")?, 0.0) {
        tree.set_native_values(rigid, "Mass", Variant::Float(0.0))?;
        log.push(format!(
            "\t{}: Changed Mass to 0.0 because of {s} collision layer",
            name(tree, rigid)?
        ));
        result = true;
    }
    if !(same_value(native_f64(tree, rigid, "Inertia Tensor\\m11")?, 0.0)
        && same_value(native_f64(tree, rigid, "Inertia Tensor\\m22")?, 0.0)
        && same_value(native_f64(tree, rigid, "Inertia Tensor\\m33")?, 0.0))
    {
        for m in ["Inertia Tensor\\m11", "Inertia Tensor\\m22", "Inertia Tensor\\m33"] {
            tree.set_native_values(rigid, m, Variant::Float(0.0))?;
        }
        log.push(format!(
            "\t{}: Changed Inertia Tensor to (0.0, 0.0, 0.0) because of {s} collision layer",
            name(tree, rigid)?
        ));
        result = true;
    }
    Ok(result)
}

/// `FixCollision`.
fn fix_collision(tree: &mut Tree, log: &mut Log) -> R<bool> {
    let mut result = false;
    let version = tree.nif.nif_version;
    if version < NifVersion::Tes4 {
        return Ok(false);
    }

    // The collision target is the parent node.
    for parent in blocks_by_type(tree, "NiAVObject", true)? {
        let Some(col) = link(tree, parent, "Collision Object")? else {
            continue;
        };
        let target = link(tree, col, "Target")?;
        if target != Some(parent) {
            let index = tree.index(parent)?;
            tree.set_native_values(col, "Target", Variant::Int(i64::from(index)))?;
            log.push(format!("\t{}: Target set to {}", name(tree, col)?, name(tree, parent)?));
            result = true;
        }

        // The target of bhkCompressedMeshShape.
        if matches!(version, NifVersion::Tes4 | NifVersion::Sse) {
            let body = link(tree, col, "Body")?;
            let Some(body) = body.filter(|&body| block_is_ni_object(tree, body, "bhkWorldObject", true)) else {
                continue;
            };
            let shape = link(tree, body, "Shape")?;
            let Some(shape) = shape.filter(|&shape| block_is_ni_object(tree, shape, "bhkMoppBvTreeShape", true)) else {
                continue;
            };
            let mesh_shape = link(tree, shape, "Shape")?;
            let Some(mesh_shape) =
                mesh_shape.filter(|&mesh| block_is_ni_object(tree, mesh, "bhkCompressedMeshShape", true))
            else {
                continue;
            };
            let target = link(tree, mesh_shape, "Target")?;
            if target != Some(parent) {
                let index = tree.index(parent)?;
                tree.set_native_values(mesh_shape, "Target", Variant::Int(i64::from(index)))?;
                log.push(format!(
                    "\t{}: Target set to {}",
                    name(tree, mesh_shape)?,
                    name(tree, parent)?
                ));
                result = true;
            }
        }
    }

    for collision in blocks_by_type(tree, "bhkCollisionObject", false)? {
        if matches!(version, NifVersion::Tes5 | NifVersion::Sse | NifVersion::Fo4)
            && !flag(tree, collision, "Flags\\SYNC_ON_UPDATE")?
        {
            set_flag(tree, collision, "Flags\\SYNC_ON_UPDATE", true)?;
            log.push(format!("\t{}: Added Sync_On_Update flag", name(tree, collision)?));
            result = true;
        }

        let body = tree
            .element_by_name(collision, "Body", true)?
            .ok_or_else(access_violation)?;
        // UPSTREAM-QUIRK: a collision object without a body ends the whole
        // fix, the rigid body settings below included.
        let Some(rigid) = tree.links_to(body)? else {
            return Ok(result);
        };

        if tree.native_values(rigid, "Havok Filter\\Layer")?.to_i64()? == 2 {
            if !flag(tree, collision, "Flags\\SET_LOCAL")? {
                set_flag(tree, collision, "Flags\\SET_LOCAL", true)?;
                log.push(format!("\t{}: Added Set_Local flag", name(tree, collision)?));
                result = true;
            }
        } else if flag(tree, collision, "Flags\\SET_LOCAL")? {
            set_flag(tree, collision, "Flags\\SET_LOCAL", false)?;
            log.push(format!("\t{}: Removed Set_Local flag", name(tree, collision)?));
            result = true;
        }
    }

    // The collision settings.
    if matches!(version, NifVersion::Fo3 | NifVersion::Tes5 | NifVersion::Sse) {
        for rigid in blocks_by_type(tree, "bhkRigidBody", true)? {
            let layer = tree.native_values(rigid, "Havok Filter\\Layer")?.to_i32()?;
            let s = tree.edit_values(rigid, "Havok Filter\\Layer")?;

            result = update_element(tree, log, rigid, "Havok Filter Copy\\Layer", &s, &s)? || result;

            if matches!(layer, 1 | 9 | 15) {
                // Static, tree and non-collidable layers.
                result = fix_fixed_body(tree, log, rigid, &s, "MO_SYS_FIXED")? || result;
            } else if version >= NifVersion::Tes5
                && layer == 2
                && tree.edit_values(rigid, "Motion System")? != "MO_SYS_KEYFRAMED"
            {
                // The animated static layer.
                result = fix_fixed_body(tree, log, rigid, &s, "MO_SYS_BOX_INERTIA")? || result;
            } else if matches!(layer, 4 | 5 | 10) {
                // Clutter, props and weapons.
                if version >= NifVersion::Tes5 {
                    result = update_element(
                        tree,
                        log,
                        rigid,
                        "Motion System",
                        "MO_SYS_SPHERE_STABILIZED",
                        "MO_SYS_SPHERE_INERTIA",
                    )? || result;
                    result = update_element(tree, log, rigid, "Motion Quality", "MO_QUAL_MOVING", "")? || result;
                } else {
                    result = update_element(
                        tree,
                        log,
                        rigid,
                        "Motion System",
                        "MO_SYS_BOX_INERTIA",
                        "MO_SYS_SPHERE_INERTIA",
                    )? || result;
                    result = update_element(tree, log, rigid, "Motion Quality", "MO_QUAL_DEBRIS", "MO_QUAL_MOVING")?
                        || result;
                }
                result = update_element(tree, log, rigid, "Deactivator Type", "DEACTIVATOR_SPATIAL", "")? || result;
                result = update_element(tree, log, rigid, "Enable Deactivation", "yes", "")? || result;
                result = update_element(
                    tree,
                    log,
                    rigid,
                    "Solver Deactivation",
                    "SOLVER_DEACTIVATION_LOW",
                    "SOLVER_DEACTIVATION_MEDIUM, SOLVER_DEACTIVATION_HIGH, SOLVER_DEACTIVATION_MAX",
                )? || result;

                if same_value(native_f64(tree, rigid, "Mass")?, 0.0) {
                    tree.set_native_values(rigid, "Mass", Variant::Float(1.0))?;
                    log.push(format!(
                        "\t{}: Changed Mass to 1.0 because of {s} collision layer and Mass was 0.0",
                        name(tree, rigid)?
                    ));
                    result = true;
                }
                if bad_tensor(native_f64(tree, rigid, "Inertia Tensor\\m11")? as f32)
                    || bad_tensor(native_f64(tree, rigid, "Inertia Tensor\\m22")? as f32)
                    || bad_tensor(native_f64(tree, rigid, "Inertia Tensor\\m33")? as f32)
                {
                    for m in ["Inertia Tensor\\m11", "Inertia Tensor\\m22", "Inertia Tensor\\m33"] {
                        tree.set_native_values(rigid, m, Variant::Float(1.0))?;
                    }
                    log.push(format!(
                        "\t{}: Changed Inertia Tensor to (1.0, 1.0, 1.0) because some values were zero or invalid",
                        name(tree, rigid)?
                    ));
                    result = true;
                }
            }

            for (factor, label) in [("Time Factor", "Time Factor"), ("Gravity Factor", "Gravity Factor")] {
                if let Some(element) = tree.elements(rigid, factor)?
                    && same_value(tree.native_value(element)?.to_f64()?, 0.0)
                {
                    tree.set_native_value(element, Variant::Float(1.0))?;
                    log.push(format!("\t{}: {label} changed from 0.0 to 1.0", name(tree, rigid)?));
                    result = true;
                }
            }

            // The layers of bhkListShape.
            let shape = link(tree, rigid, "Shape")?;
            if let Some(shape) = shape
                && block_type(tree, shape) == "bhkListShape"
            {
                let sub_shapes = el(tree, shape, "Sub Shapes")?;
                let filters = el(tree, shape, "Filters")?;
                let count = tree.count(sub_shapes);
                tree.set_count(filters, count)?;

                let mut updated = false;
                for i in (0..count).rev() {
                    let item = tree.item(sub_shapes, i)?;
                    if tree.links_to(item)?.is_none() {
                        tree.delete(sub_shapes, i)?;
                        tree.delete(filters, i)?;
                        updated = true;
                        continue;
                    }
                    let filter = tree.item(filters, i)?;
                    if tree.native_values(filter, "Layer")?.to_i64()? == 0 {
                        tree.set_native_values(filter, "Layer", Variant::Int(i64::from(layer)))?;
                        updated = true;
                    }
                }
                if updated {
                    log.push(format!(
                        "\t{}: Updated Filters to match the rigid body layer",
                        name(tree, shape)?
                    ));
                    result = true;
                }
            }
        }
    }
    Ok(result)
}

// ===========================================================================
/// `FixParticles`.
fn fix_particles(tree: &mut Tree, log: &mut Log) -> R<bool> {
    let mut result = false;
    if tree.nif.nif_version != NifVersion::Sse {
        return Ok(false);
    }
    for b in blocks_by_type(tree, "NiPSysMeshEmitter", true)? {
        let meshes = el(tree, b, "Emitter Meshes")?;
        for i in 0..tree.count(meshes) {
            let item = tree.item(meshes, i)?;
            let Some(shape) = tree.links_to(item)? else { continue };
            if block_type(tree, shape) != "BSTriShape" {
                continue;
            }
            if tree.native_values(shape, "Particle Data Size")?.to_i64()? != 0 {
                continue;
            }
            // Enables the particle data; the save sets the right size.
            tree.set_native_values(shape, "Particle Data Size", Variant::Int(1))?;

            let Some(vertices) = tree.elements(shape, "Vertex Data")? else {
                continue;
            };
            let count = tree.count(vertices);
            if count == 0 {
                continue;
            }
            let p_vertices = el(tree, shape, "Particle Vertices")?;
            let p_normals = el(tree, shape, "Particle Normals")?;
            tree.set_count(p_vertices, count)?;
            tree.set_count(p_normals, count)?;
            for j in 0..count {
                let vertex = tree.item(vertices, j)?;
                let position = tree.edit_values(vertex, "Vertex")?;
                let normal = tree.edit_values(vertex, "Normal")?;
                let p_vertex = tree.item(p_vertices, j)?;
                tree.set_edit_value(p_vertex, &position)?;
                let p_normal = tree.item(p_normals, j)?;
                tree.set_edit_value(p_normal, &normal)?;
            }
            let triangles = tree.elements(shape, "Triangles")?;
            let particle_triangles = el(tree, shape, "Particle Triangles")?;
            tree.assign(particle_triangles, triangles)?;

            log.push(format!(
                "\t{}: Added missing Particle Data for mesh emitter used by {}",
                name(tree, shape)?,
                name(tree, b)?
            ));
            result = true;
        }
    }
    Ok(result)
}

// ===========================================================================
/// `FixConsistencyFlags`.
fn fix_consistency_flags(tree: &mut Tree, log: &mut Log) -> R<bool> {
    let mut result = false;
    if !matches!(
        tree.nif.nif_version,
        NifVersion::Tes4 | NifVersion::Fo3 | NifVersion::Tes5
    ) {
        return Ok(false);
    }
    for shape in blocks_by_type(tree, "NiGeometry", true)? {
        let Some(data) = link(tree, shape, "Data")? else {
            continue;
        };
        if tree.elements(data, "Consistency Flags")?.is_none() {
            continue;
        }
        let controller = block_get_controller(tree, shape, "", false)?;
        let mutable = block_is_ni_object(tree, shape, "NiParticles", true)
            || controller.is_some_and(|controller| {
                block_is_ni_object(tree, controller, "NiGeomMorpherController", true)
                    || block_is_ni_object(tree, controller, "NiUVController", true)
            });
        let f = if mutable { "CT_MUTABLE" } else { "CT_STATIC" };
        let current = tree.edit_values(data, "Consistency Flags")?;
        if current != f {
            log.push(format!(
                "\t{}: Consistency Flags changed from {current} to {f}",
                name(tree, data)?
            ));
            tree.set_edit_values(data, "Consistency Flags", f)?;
            result = true;
        }
    }
    Ok(result)
}

// ===========================================================================
/// `FixRedundantBlocks`.
fn fix_redundant_blocks(tree: &mut Tree, log: &mut Log) -> R<bool> {
    let mut result = false;

    // Empty shapes.
    for i in (0..blocks_count(tree)?).rev() {
        // Several blocks may have gone already. UPSTREAM-QUIRK: `>` lets
        // the index equal to the count through, which fails.
        if i > blocks_count(tree)? {
            continue;
        }
        let shape = block(tree, i)?;
        let mut verts: i64 = 0;
        if block_is_ni_object(tree, shape, "BSTriShape", true) {
            if link(tree, shape, "Skin")?.is_some() {
                continue;
            }
            verts = tree.native_values(shape, "Num Vertices")?.to_i64()?;
        } else if block_is_ni_object(tree, shape, "NiTriBasedGeom", true) {
            if link(tree, shape, "Skin Instance")?.is_some() {
                continue;
            }
            if let Some(shape_data) = link(tree, shape, "Data")? {
                verts = tree.native_values(shape_data, "Num Vertices")?.to_i64()?;
            }
        } else {
            continue;
        }
        if verts != 0 {
            continue;
        }
        if !block_extra_datas_by_type(tree, shape, "NiExtraData", true)?.is_empty() {
            continue;
        }
        log.push(format!("\t{}: Removed empty geometry shape", name(tree, shape)?));
        block_remove_branch(tree, shape, true)?;
        result = true;
    }

    // NiStringExtraData "UPB": Fallout 3 and later check it only on a node
    // named Backpack, naming the block to parent the backpack to.
    for b in blocks_by_type(tree, "NiStringExtraData", false)? {
        if tree.edit_values(b, "Name")? != "UPB" {
            continue;
        }
        let mut backpack = false;
        for reference in block_referenced_by(tree, b)? {
            if let Some(parent) = nifblk(tree, reference)
                && tree.edit_values(parent, "Name")? == "Backpack"
            {
                backpack = true;
                break;
            }
        }
        if !backpack {
            log.push(format!("\t{}: Removed unused \"UPB\" extra data", name(tree, b)?));
            block_remove_branch(tree, b, true)?;
            result = true;
        }
    }

    // A useless NiSpecularProperty.
    if tree.nif.nif_version >= NifVersion::Tes4 {
        for spec in blocks_by_type(tree, "NiSpecularProperty", false)? {
            log.push(format!(
                "\t{}: Removed because redundant and does nothing",
                name(tree, spec)?
            ));
            block_remove_branch(tree, spec, true)?;
            result = true;
        }
    }
    Ok(result)
}

// ===========================================================================
/// The shader types of Skyrim and their texture slots and flags, in the
/// order `FixShaderProperty` decides on them.
struct ShaderTextures {
    emissive: bool,
    parallax: bool,
    env_mapped: bool,
    sub_surface: bool,
}

/// A flag that a shader type adds and the other types remove.
fn toggle_flag(
    tree: &mut Tree,
    log: &mut Log,
    shader: El,
    on: bool,
    path: &str,
    added: &str,
    removed: &str,
) -> R<bool> {
    let set = flag(tree, shader, path)?;
    if on && !set {
        set_flag(tree, shader, path, true)?;
        log.push(format!("\t{}: {added}", name(tree, shader)?));
        return Ok(true);
    }
    if !on && set {
        set_flag(tree, shader, path, false)?;
        log.push(format!("\t{}: {removed}", name(tree, shader)?));
        return Ok(true);
    }
    Ok(false)
}

/// A flag that is only added.
fn add_flag(tree: &mut Tree, log: &mut Log, shader: El, path: &str, message: &str) -> R<bool> {
    if !flag(tree, shader, path)? {
        set_flag(tree, shader, path, true)?;
        log.push(format!("\t{}: {message}", name(tree, shader)?));
        return Ok(true);
    }
    Ok(false)
}

/// A flag that is only removed.
fn remove_flag(tree: &mut Tree, log: &mut Log, shader: El, path: &str, message: &str) -> R<bool> {
    if flag(tree, shader, path)? {
        set_flag(tree, shader, path, false)?;
        log.push(format!("\t{}: {message}", name(tree, shader)?));
        return Ok(true);
    }
    Ok(false)
}

/// The shader type the textures and flags of a `BSLightingShaderProperty`
/// ask for, or `""` when undecided.
fn decide_shader_type(tree: &mut Tree, shader: El, shader_type: &str, t: &ShaderTextures) -> R<&'static str> {
    let facegen = flag(tree, shader, "Shader Flags 1\\Facegen")?;
    let multi_layer = flag(tree, shader, "Shader Flags 2\\Multi_Layer_Parallax")?;
    let env = flag(tree, shader, "Shader Flags 1\\Environment_Mapping")?;
    let eye_env = flag(tree, shader, "Shader Flags 1\\Eye_Environment_Mapping")?;
    let glow = flag(tree, shader, "Shader Flags 2\\Glow_Map")?;
    let skin_tint = flag(tree, shader, "Shader Flags 1\\Skin_Tint")?;
    let parallax = flag(tree, shader, "Shader Flags 1\\Parallax")?;
    // Each type with the textures it needs and its flag.
    let types: [(&'static str, bool, bool); 7] = [
        ("Facegen", t.emissive && t.parallax && t.sub_surface, facegen),
        ("MultiLayer Parallax", t.env_mapped && t.sub_surface, multi_layer),
        ("Environment Map", t.env_mapped, env),
        ("Eye Envmap", t.env_mapped, eye_env),
        ("Glow Shader", t.emissive, glow),
        ("Skin Tint", t.emissive, skin_tint),
        ("Parallax", t.parallax, parallax),
    ];
    // Perfect matches first: type, textures and flag.
    for (name, textures, flag) in types {
        if shader_type == name && textures && flag {
            return Ok(name);
        }
    }
    // Less perfect: type and textures.
    for (name, textures, _) in types {
        if shader_type == name && textures {
            return Ok(name);
        }
    }
    // Even less perfect: textures and flag.
    for (name, textures, flag) in types {
        if textures && flag {
            return Ok(name);
        }
    }
    Ok("")
}

/// `FixShaderProperty`.
fn fix_shader_property(tree: &mut Tree, root: El, log: &mut Log) -> R<bool> {
    let mut result = false;
    let version = tree.nif.nif_version;
    if !matches!(
        version,
        NifVersion::Tes4 | NifVersion::Fo3 | NifVersion::Tes5 | NifVersion::Sse | NifVersion::Fo4
    ) {
        return Ok(false);
    }

    let facegen_nif = block_by_name(tree, "BSFaceGenNiNodeSkinned", "NiNode")?.is_some();

    for i in 0..blocks_count(tree)? {
        let shape = block(tree, i)?;
        if !(block_is_ni_object(tree, shape, "BSTriShape", true) || block_is_ni_object(tree, shape, "NiGeometry", true))
        {
            continue;
        }
        let Some(shader) = block_property_by_type(tree, shape, "BSShaderProperty", true)? else {
            continue;
        };
        let texset = match tree.elements(shader, "Texture Set")? {
            Some(texture_set) => tree.links_to(texture_set)?,
            None => None,
        };

        // The Skinned flag.
        let skinned = block_get_skin(tree, shape)?.is_some();
        result = toggle_flag(
            tree,
            log,
            shader,
            skinned,
            "Shader Flags 1\\Skinned",
            "Added Skinned flag",
            "Removed Skinned flag",
        )? || result;

        // The Vertex Colors flag.
        let mut has_vertex_colors = false;
        if block_is_ni_object(tree, shape, "NiGeometry", true) {
            if let Some(shape_data) = link(tree, shape, "Data")? {
                has_vertex_colors = flag(tree, shape_data, "Has Vertex Colors")?;
            }
        } else {
            has_vertex_colors = flag(tree, shape, "VertexDesc\\VF\\VF_COLORS")?;
        }

        // Set at run time in Fallout 3 and New Vegas.
        if version != NifVersion::Fo3 {
            if has_vertex_colors {
                result = add_flag(
                    tree,
                    log,
                    shader,
                    "Shader Flags 2\\Vertex_Colors",
                    "Added Vertex_Colors flag because vertex colors are present",
                )? || result;
            } else {
                result = remove_flag(
                    tree,
                    log,
                    shader,
                    "Shader Flags 2\\Vertex_Colors",
                    "Removed Vertex_Colors flag because vertex colors are missing",
                )? || result;
            }
        }

        if !has_vertex_colors {
            result = remove_flag(
                tree,
                log,
                shader,
                "Shader Flags 1\\Vertex_Alpha",
                "Removed Vertex_Alpha flag because vertex colors are missing",
            )? || result;
        }

        // The shader type and flags of Skyrim.
        if matches!(version, NifVersion::Tes5 | NifVersion::Sse) {
            // The Dynamic_Decal flag.
            if flag(tree, shader, "Shader Flags 1\\Dynamic_Decal")? {
                result = add_flag(
                    tree,
                    log,
                    shader,
                    "Shader Flags 1\\Decal",
                    "Added Decal flag because Dynamic_Decal flag is used",
                )? || result;
                result = add_flag(
                    tree,
                    log,
                    shader,
                    "Shader Flags 2\\Assume_Shadowmask",
                    "Added Assume_Shadowmask flag because Dynamic_Decal flag is used",
                )? || result;
            }

            if block_type(tree, shader) == "BSLightingShaderProperty" {
                let Some(texset) = texset else { continue };
                let shader_type = tree.edit_values(shader, "Shader Type")?;
                let mut texture = |index: usize| -> R<bool> {
                    Ok(!tree.edit_values(texset, &format!("Textures\\[{index}]"))?.is_empty())
                };
                let textures = ShaderTextures {
                    emissive: texture(2)?,
                    parallax: texture(3)?,
                    env_mapped: texture(4)? || texture(5)?,
                    sub_surface: texture(6)?,
                };
                // An empty type is undecided.
                let new_shader_type = decide_shader_type(tree, shader, &shader_type, &textures)?;
                if !new_shader_type.is_empty() && shader_type != new_shader_type {
                    tree.set_edit_values(shader, "Shader Type", new_shader_type)?;
                    log.push(format!(
                        "\t{}: Changed Shader Type from {shader_type} to {new_shader_type} based on assigned textures and flags",
                        name(tree, shader)?
                    ));
                    result = true;
                }

                let shader_type = tree.edit_values(shader, "Shader Type")?;

                result = toggle_flag(
                    tree,
                    log,
                    shader,
                    shader_type == "Environment Map",
                    "Shader Flags 1\\Environment_Mapping",
                    "Added Environment_Mapping flag because Shader Type is Environment Map",
                    "Removed Environment_Mapping flag because Shader Type is not Environment Map",
                )? || result;

                if shader_type == "Glow Shader" {
                    result = add_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 2\\Glow_Map",
                        "Added Glow_Map flag because Shader Type is Glow Shader",
                    )? || result;
                    result = add_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 1\\Own_Emit",
                        "Added Own_Emit flag because Shader Type is Glow Shader",
                    )? || result;
                } else {
                    result = remove_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 2\\Glow_Map",
                        "Removed Glow_Map flag because Shader Type is not Glow Shader",
                    )? || result;
                    // Own_Emit goes when the emissive color is black.
                    if flag(tree, shader, "Shader Flags 1\\Own_Emit")?
                        && tree.edit_values(shader, "Emissive Color")? == "#000000"
                    {
                        set_flag(tree, shader, "Shader Flags 1\\Own_Emit", false)?;
                        log.push(format!(
                            "\t{}: Removed Own_Emit flag because Emissive Color is Blank",
                            name(tree, shader)?
                        ));
                        result = true;
                    }
                    // External_Emittance does not work without glow.
                    result = remove_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 1\\External_Emittance",
                        "Removed External_Emittance flag because Shader Type is not Glow Shader",
                    )? || result;
                }

                result = toggle_flag(
                    tree,
                    log,
                    shader,
                    shader_type == "Parallax",
                    "Shader Flags 1\\Parallax",
                    "Added Parallax flag because Shader Type is Parallax",
                    "Removed Parallax flag because Shader Type is not Parallax",
                )? || result;

                if shader_type == "Facegen" {
                    result = add_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 1\\Facegen",
                        "Added Facegen flag because Shader Type is Facegen",
                    )? || result;
                    result = add_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 2\\Soft_Lighting",
                        "Added Soft_Lighting flag because Shader Type is Facegen",
                    )? || result;
                    // Anisotropic_Lighting does not go with the Facegen shader.
                    result = remove_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 2\\Anisotropic_Lighting",
                        "Removed Anisotropic_Lighting flag because Shader Type is Facegen",
                    )? || result;
                } else {
                    result = remove_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 1\\Facegen",
                        "Removed Facegen flag because Shader Type is not Facegen",
                    )? || result;
                }

                if shader_type == "Skin Tint" {
                    result = add_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 1\\Skin_Tint",
                        "Added Skin_Tint flag because Shader Type is Skin Tint",
                    )? || result;
                    result = add_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 2\\Soft_Lighting",
                        "Added Soft_Lighting flag because Shader Type is Skin Tint",
                    )? || result;
                } else {
                    result = remove_flag(
                        tree,
                        log,
                        shader,
                        "Shader Flags 1\\Skin_Tint",
                        "Removed Skin_Tint flag because Shader Type is not Skin Tint",
                    )? || result;
                }

                result = toggle_flag(
                    tree,
                    log,
                    shader,
                    shader_type == "Hair Tint",
                    "Shader Flags 1\\Hair_Tint",
                    "Added Hair_Tint flag because Shader Type is Hair Tint",
                    "Removed Hair_Tint flag because Shader Type is not Hair Tint",
                )? || result;

                result = toggle_flag(
                    tree,
                    log,
                    shader,
                    shader_type == "MultiLayer Parallax",
                    "Shader Flags 2\\Multi_Layer_Parallax",
                    "Added Multi_Layer_Parallax flag because Shader Type is MultiLayer Parallax",
                    "Removed Multi_Layer_Parallax flag because Shader Type is not MultiLayer Parallax",
                )? || result;

                result = toggle_flag(
                    tree,
                    log,
                    shader,
                    shader_type == "Eye Envmap",
                    "Shader Flags 1\\Eye_Environment_Mapping",
                    "Added Eye_Environment_Mapping flag because Shader Type is Eye EnvMap",
                    "Removed Eye_Environment_Mapping flag because Shader Type is not Eye Envmap",
                )? || result;

                // The Character_Lighting flag.
                let nif_name = name(tree, root)?;
                result = toggle_flag(
                    tree,
                    log,
                    shader,
                    facegen_nif,
                    "Shader Flags 2\\Character_Lighting",
                    &format!("Added Character_Lighting flag because {nif_name} is a Facegen nif"),
                    &format!("Removed Character_Lighting flag because {nif_name} is not a Facegen nif"),
                )? || result;

                // The EnvMap_Light_Fade flag.
                result = toggle_flag(
                    tree,
                    log,
                    shader,
                    shader_type == "Environment Map"
                        || shader_type == "MultiLayer Parallax"
                        || shader_type == "Eye Envmap",
                    "Shader Flags 2\\EnvMap_Light_Fade",
                    "Added EnvMap_Light_Fade flag because Shader Type is Environment/MultiLayer Parallax",
                    "Removed EnvMap_Light_Fade flag because Shader Type is not Environment/MultiLayer Parallax",
                )? || result;

                // The Specular flag.
                if flag(tree, shader, "Shader Flags 1\\Specular")?
                    && tree.edit_values(shader, "Specular Color")? == "#000000"
                {
                    set_flag(tree, shader, "Shader Flags 1\\Specular", false)?;
                    log.push(format!(
                        "\t{}: Removed Specular flag because Specular Color is Blank",
                        name(tree, shader)?
                    ));
                    result = true;
                }

                // The Tree_Anim flag.
                if flag(tree, shader, "Shader Flags 2\\Tree_Anim")? {
                    let root_node = root_node(tree)?;
                    let root_type = block_type(tree, root_node);
                    if root_type != "BSLeafAnimNode" && root_type != "BSTreeNode" {
                        set_flag(tree, shader, "Shader Flags 2\\Tree_Anim", false)?;
                        if flag(tree, shader, "Shader Flags 1\\Vertex_Alpha")? {
                            set_flag(tree, shader, "Shader Flags 1\\Vertex_Alpha", false)?;
                        }
                        log.push(format!(
                            "\t{}: Removed Tree_Anim flag because root node is not BSLeafAnimNode or BSTreeNode",
                            name(tree, shader)?
                        ));
                        result = true;
                    }
                    if shader_type != "Default" && shader_type != "Tree Anim" {
                        set_flag(tree, shader, "Shader Flags 2\\Tree_Anim", false)?;
                        if flag(tree, shader, "Shader Flags 1\\Vertex_Alpha")? {
                            set_flag(tree, shader, "Shader Flags 1\\Vertex_Alpha", false)?;
                        }
                        log.push(format!(
                            "\t{}: Removed Tree_Anim flag because of wrong Shader Type",
                            name(tree, shader)?
                        ));
                        result = true;
                    }
                }

                if flag(tree, shader, "Shader Flags 2\\Tree_Anim")? {
                    if has_vertex_colors {
                        result = add_flag(
                            tree,
                            log,
                            shader,
                            "Shader Flags 1\\Vertex_Alpha",
                            "Added Vertex_Alpha flag because Tree_Anim flag is set",
                        )? || result;
                    } else {
                        set_flag(tree, shader, "Shader Flags 2\\Tree_Anim", false)?;
                        log.push(format!(
                            "\t{}: Removed Tree_Anim flag because vertex colors are missing",
                            name(tree, shader)?
                        ));
                        result = true;
                    }
                }

                if tree.native_values(shader, "Glossiness")?.to_f64()? == 0.0 {
                    tree.set_native_values(shader, "Glossiness", Variant::Int(1))?;
                    log.push(format!(
                        "\t{}: Set Glossiness to 1, because 0 causes lighting issues",
                        name(tree, shader)?
                    ));
                    result = true;
                }
            }
        }
        // Fallout 4: an external material file makes the shader settings
        // unused; nothing follows.
    }
    Ok(result)
}

impl Proc for ProcUniversalFixer {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.save_log = storage.get_bool("bSaveLog", false);
        self.open_log = storage.get_bool("bOpenLog", false);
        self.log_file_name = storage.get_string("sLogFile", "");
    }

    fn on_start(&mut self) -> R<()> {
        if self.save_log {
            if self.log_file_name.is_empty() {
                return Err(DfError::new("Select log file name"));
            }
            let dir = extract_file_path(&self.log_file_name);
            if !dir.is_empty() && !std::path::Path::new(dir).is_dir() {
                return Err(DfError::new(format!("Log file folder doesn't exist: {dir}")));
            }
        }
        Ok(())
    }

    fn on_stop(&mut self, ctx: &mut StopContext) -> R<()> {
        if !self.save_log || ctx.dry_run {
            return Ok(());
        }
        std::fs::write(&self.log_file_name, string_list_file_bytes(ctx.proc_log))
            .map_err(|error| DfError::new(error.to_string()))?;
        // `bOpenLog` opens the log in the shell; the port leaves it.
        let _ = self.open_log;
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut log: Log = Vec::new();
        let mut nif = NifFile::new()?;
        nif.tree.nif.options = NifOptions {
            collapse_link_arrays: true,
            remove_unused_strings: true,
        };
        nif.load_from_data(&file.get_data()?)?;
        let root = nif.root;
        let tree = &mut nif.tree;

        let mut changed = false;
        changed = fix_string_indices(tree, &mut log)? || changed;
        changed = fix_array_links(tree, &mut log)? || changed;
        // The empty geometry after the link arrays, so the empty links left
        // are not reported; the save collapses them.
        changed = fix_redundant_blocks(tree, &mut log)? || changed;
        changed = fix_absolute_paths(tree, &mut log)? || changed;
        // The shaders before BSXFlags: External_Emittance depends on them.
        changed = fix_shader_property(tree, root, &mut log)? || changed;
        changed = fix_bsx_flags(tree, &mut log)? || changed;
        changed = fix_hardcoded_names(tree, &mut log)? || changed;
        changed = fix_anim(tree, &mut log)? || changed;
        changed = fix_collision(tree, &mut log)? || changed;
        changed = fix_particles(tree, &mut log)? || changed;
        changed = fix_consistency_flags(tree, &mut log)? || changed;

        if changed {
            let result = nif.save_to_data()?;
            ctx.proc_log.push(file.file_name.clone());
            ctx.proc_log.extend(log);
            ctx.proc_log.push(String::new());
            return Ok(result);
        }
        Ok(Vec::new())
    }
}
