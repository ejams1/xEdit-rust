// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbLOD.pas (wbFindREFRs,
// wbFindUniqueWorldspaceREFRs, wbGetLODMeshName, wbGenerateLODTES4,
// wbGenerateLODTES5, wbGenerateLODFO4), and the worldspace list of
// xEdit/xeMainForm.pas (mniNavGenerateLODClick, DoGenerateLOD)

//! The LOD of a worldspace: the references of the worldspace and its
//! overrides, the distant LOD files of Oblivion, and the tree and objects
//! LOD of Skyrim, the Fallouts and Fallout 4.

use std::sync::Arc;

use xedit_core::delphi::{float_to_str, float_to_str_f_fixed};
use xedit_core::implementation::{FileImpl, MainRecordImpl};
use xedit_core::interface::element::{Container, Element, ElementRef, MainRecordRef};
use xedit_core::interface::form_id::{FileID, FormID};
use xedit_core::interface::globals::{
    GameMode, app_name, cell_size_factor, game_mode, is_fallout3, is_fallout4, is_skyrim, is_starfield,
};
use xedit_core::interface::types::Signature;
use xedit_io::archive::{AssetType, get_asset_name};

use super::atlas::{build_atlas_from_textures_list, get_uv_range_textures_list, load_material};
use super::trees::{LodSettings, TreeBlock, TreeList, load_tree_image};
use super::{
    LODGEN_NAME, ListCompare, LodEnv, LodError, LodResult, StringList, boolean_text, change_file_ext, delimited_text,
    extract_file_name, extract_file_path, force_directories, lod_extra_options_file_name, lod_settings_file_name,
    lod_tree_block_file_ext, matches_mask, message, open_resource, resource_exists, trim,
};
use crate::sniff::processor::MemIniFile;

const REFR: Signature = Signature::new(b"REFR");
const WRLD: Signature = Signature::new(b"WRLD");

/// The implementation of a record.
fn record_impl(record: &MainRecordRef) -> Option<Arc<MainRecordImpl>> {
    record.as_element_impl().and_then(|element| element.main_record_impl())
}

/// The elements of a container.
fn children(element: &dyn Element) -> Vec<ElementRef> {
    match element.as_container() {
        Some(container) => (0..container.get_element_count())
            .filter_map(|index| container.get_element(index))
            .collect(),
        None => Vec::new(),
    }
}

/// `wbFindREFRs`: the `REFR` records below a group, in the order of the
/// groups.
fn find_refrs(element: &ElementRef, refrs: &mut Vec<MainRecordRef>) {
    if let Some(record) = element.clone().into_main_record() {
        if record.get_signature() == REFR {
            refrs.push(record);
        }
    } else if element.as_container().is_some() {
        for child in children(&**element) {
            find_refrs(&child, refrs);
        }
    }
}

/// `CompareElementsFormIDAndLoadOrder`.
fn compare_form_id_and_load_order(a: &MainRecordRef, b: &MainRecordRef) -> std::cmp::Ordering {
    FormID::compare(a.get_load_order_form_id(), b.get_load_order_form_id()).then_with(|| {
        let order = |record: &MainRecordRef| record.get_file().map_or(0, |file| file.get_load_order());
        order(a).cmp(&order(b))
    })
}

/// Keeps the last of each run of equal load order FormIDs of a sorted list.
fn keep_newest(records: &mut Vec<MainRecordRef>) {
    if records.len() <= 1 {
        return;
    }
    let mut j = 0;
    for i in 1..records.len() {
        if records[j].get_load_order_form_id() != records[i].get_load_order_form_id() {
            j += 1;
        }
        if j != i {
            records[j] = records[i].clone();
        }
    }
    records.truncate(j + 1);
}

/// `wbFindUniqueWorldspaceREFRs`: the references of the worldspace and of
/// its overrides, the newest version of each, sorted by FormID.
pub fn find_unique_worldspace_refrs(worldspace: &MainRecordRef) -> Vec<MainRecordRef> {
    let mut refrs = Vec::new();
    let master = worldspace.get_master_or_self();
    if let Some(master) = record_impl(&master) {
        if let Some(group) = master.child_group() {
            find_refrs(&(group as ElementRef), &mut refrs);
        }
        for over in master.overrides() {
            if let Some(group) = over.child_group() {
                find_refrs(&(group as ElementRef), &mut refrs);
            }
        }
    }
    // only keep the newest version of each (`wbMergeSortPtr` is stable)
    if refrs.len() > 1 {
        refrs.sort_by(compare_form_id_and_load_order);
        keep_newest(&mut refrs);
    }
    refrs
}

/// The worldspaces of the loaded files that get LOD (`DoGenerateLOD` for
/// Oblivion, `mniNavGenerateLODClick` for the others: the ones with a LOD
/// settings file and without the `Use LOD Data` flag of a parent world),
/// sorted by FormID with the newest version of each.
pub fn worldspaces_for_lod(files: &[Arc<FileImpl>]) -> Vec<MainRecordRef> {
    let mut worldspaces = Vec::new();
    let needs_settings = is_skyrim() || is_fallout3() || is_fallout4() || is_starfield();
    for file in files {
        let Some(group) = file.group_by_signature(WRLD) else {
            continue;
        };
        for element in children(&*group) {
            let Some(record) = element.into_main_record() else {
                continue;
            };
            if record.get_signature() != WRLD {
                continue;
            }
            if needs_settings {
                if !resource_exists(&lod_settings_file_name(&record.get_editor_id())) {
                    continue;
                }
                // do not list worldspace if Use LOD Data flag of parent world is set
                if record.get_element_exists("Parent\\WNAM")
                    && record
                        .get_element_native_value("Parent\\PNAM\\Flags")
                        .as_ordinal()
                        .unwrap_or(0)
                        & 2
                        == 2
                {
                    continue;
                }
            }
            worldspaces.push(record);
        }
    }
    if worldspaces.len() > 1 {
        worldspaces.sort_by(compare_form_id_and_load_order);
        keep_newest(&mut worldspaces);
    }
    worldspaces
}

/// `IntToHex(value, 8)`.
fn hex8(value: u32) -> String {
    format!("{value:08X}")
}

/// `Flags._Flags` of a record.
fn flags(record: &MainRecordRef) -> u32 {
    record.get_flags().0
}

const FLAG_INITIALLY_DISABLED: u32 = 0x0000_0800;

/// `GetPosition`: the position of a placed object, from `DATA`.
fn get_position(record: &MainRecordRef) -> Option<(f32, f32, f32)> {
    let data = data_struct(record)?;
    let position = data.as_container()?.get_element(0)?;
    let position = position.as_container()?;
    if position.get_element_count() != 3 {
        return None;
    }
    let value = |i: i32| {
        position
            .get_element(i)
            .map_or(0.0, |e| e.get_native_value().as_number().unwrap_or(0.0)) as f32
    };
    Some((value(0), value(1), value(2)))
}

/// `GetRotation`: the rotation of a placed object in degrees, from the
/// values of `DATA`.
fn get_rotation(record: &MainRecordRef) -> Option<(f32, f32, f32)> {
    let data = data_struct(record)?;
    let rotation = data.as_container()?.get_element(1)?;
    let rotation = rotation.as_container()?;
    if rotation.get_element_count() != 3 {
        return None;
    }
    let value = |i: i32| {
        rotation
            .get_element(i)
            .and_then(|e| xedit_core::delphi::str_to_float(&e.get_value()))
            .unwrap_or(0.0) as f32
    };
    Some((value(0), value(1), value(2)))
}

/// The `DATA` of a placed object with its two members.
fn data_struct(record: &MainRecordRef) -> Option<ElementRef> {
    let signature = record.get_signature();
    const PLACED: [&[u8; 4]; 11] = [
        b"REFR", b"ACRE", b"ACHR", b"PGRE", b"PMIS", b"PARW", b"PBEA", b"PFLA", b"PCON", b"PBAR", b"PHZD",
    ];
    if !PLACED.iter().any(|placed| signature == Signature::new(placed)) {
        return None;
    }
    let data = record.get_record_by_signature(Signature::new(b"DATA"))?;
    (data.as_container()?.get_element_count() == 2).then_some(data)
}

/// `wbPositionToGridCell`.
fn position_to_grid_cell(pos: (f32, f32, f32)) -> (i32, i32) {
    let factor = cell_size_factor();
    let axis = |value: f32| {
        let q = f64::from(value) / factor;
        let mut result = q.trunc() as i32;
        if value < 0.0 && q.fract() != 0.0 {
            result -= 1;
        }
        result
    };
    (axis(pos.0), axis(pos.1))
}

/// `wbGetLODMeshName`: the full model (level -1) or the LOD model of a
/// level, if the resources have it.
///
/// UPSTREAM-QUIRK: the LOD models of Skyrim and Fallout 4 are read from
/// `MNAM\LOD #<n> (Level <n>)\Mesh`, a path the definitions of the release
/// do not have (`MNAM` holds `Level 0` to `Level 3` there), so a static with
/// `MNAM` never has a LOD model.
pub fn get_lod_mesh_name(stat: &MainRecordRef, lod_level: i32, trees_3d: bool) -> String {
    let mut result = String::new();
    if lod_level == -1 {
        result = stat.get_element_edit_value("Model\\MODL");
    } else if is_skyrim() || is_fallout4() {
        if stat.get_element_exists("MNAM") {
            result = stat.get_element_edit_value(&format!("MNAM\\LOD #{lod_level} (Level {lod_level})\\Mesh"));
        } else if trees_3d && stat.get_signature() == Signature::new(b"TREE") {
            result = format!(
                "{}_lod_{lod_level}.nif",
                change_file_ext(&stat.get_element_edit_value("Model\\MODL"), "")
            );
        }
    } else if is_fallout3() && lod_level == 0 {
        result = format!(
            "{}_lod.nif",
            change_file_ext(&stat.get_element_edit_value("Model\\MODL"), "")
        );
    }
    let result = get_asset_name(&result, "", AssetType::Mesh);
    if lod_level != -1 && !resource_exists(&result) {
        return String::new();
    }
    result
}

/// The list of archives of the export file (`Resource=`): every container
/// but the last, the data folder.
fn resource_lines(export: &mut StringList) {
    let containers = xedit_core::container_handler::container_list();
    for container in containers.iter().take(containers.len().saturating_sub(1)) {
        export.add(&format!("Resource={container}"));
    }
}

/// `wbLODExtraOptionsFileName` added to the export file when it exists.
fn extra_options(env: &LodEnv, worldspace: &MainRecordRef, export: &mut StringList) {
    let id = worldspace.get_editor_id();
    let plugin = worldspace
        .get_master_or_self()
        .get_file()
        .map(|file| change_file_ext(extract_file_name(&file.get_name()), ""))
        .unwrap_or_default();
    let s = format!("{}{}", env.scripts_path, lod_extra_options_file_name(&plugin, &id));
    if std::path::Path::new(&s).is_file() {
        if let Ok(list) = StringList::load_from_file(&s) {
            for line in list.strings() {
                export.add(line);
            }
        }
        message(&format!("[{id}] Using options file: {s}"));
    } else {
        message(&format!("[{id}] No options file found: {s}"));
    }
}

/// Runs `LODGenx64.exe` on the export file: an exception with its exit code
/// when that is not 0.
fn run_lodgen(command: &str) -> LodResult<()> {
    let code = xedit_core::helpers::execute_capture_console_output(command)
        .map_err(|error| LodError::new(format!("{error}")))?;
    if code != 0 {
        return Err(LodError::new(format!("LODGen process error, exit code {code:08X}")));
    }
    Ok(())
}

/// The name of a record as the messages give it.
fn name(record: &MainRecordRef) -> String {
    record.get_name()
}

// ---------------------------------------------------------------------------
// Oblivion

/// `TRefInfo`.
#[derive(Debug, Clone, Copy, Default)]
struct RefInfo {
    form_id: FormID,
    pos: (f32, f32, f32),
    rot: (f32, f32, f32),
    scale: f32,
}

/// `HasVisibleWhenDistantMesh`: a tree's billboard or the `_far.nif` of a
/// model.
fn has_visible_when_distant_mesh(record: &MainRecordRef) -> bool {
    let Some(model) = record.get_element_by_name("Model") else {
        return false;
    };
    let Some(modl) = model
        .as_container()
        .and_then(|c| c.get_record_by_signature(Signature::new(b"MODL")))
    else {
        return false;
    };
    let s = trim(&modl.get_edit_value().replace('/', "\\")).to_owned();
    if s.is_empty() {
        return false;
    }
    let s = if record.get_signature() == Signature::new(b"TREE") {
        format!("textures\\trees\\billboards{}", change_file_ext(&s, ".dds"))
    } else {
        format!("meshes\\{}", change_file_ext(&s, "_far.nif"))
    };
    !xedit_core::container_handler::open_resource(&s).is_empty()
}

/// `RoundTo(Scale, -2)` of a single.
fn round_to_2(value: f32) -> f32 {
    crate::nif_math::round_to(f64::from(value), -2) as f32
}

/// `wbGenerateLODTES4`: the `.lod` files of the cells of the worldspace
/// and its `.cmp` file below `DistantLOD\` of the output folder, by the
/// rule of the settings (`[Worldspace] <EditorID>`, else `[Default] Rule`).
pub fn generate_lod_tes4(env: &LodEnv, worldspace: &MainRecordRef, elapsed: &dyn Fn() -> String) -> LodResult<()> {
    let lod_scale: f32 = 100.0;
    let lod_add: f32 = 0.970_001_2;
    let master = worldspace.get_master_or_self();
    let mut s = env.read_string("Worldspace", &master.get_editor_id(), "");
    if s.is_empty() {
        s = env.read_string("Default", "Rule", "Replace");
    }
    #[derive(PartialEq, PartialOrd)]
    enum Rule {
        Skip,
        Clear,
        Replace,
    }
    let (rule, action) = if s.eq_ignore_ascii_case("Replace") {
        (Rule::Replace, "Replacing")
    } else if s.eq_ignore_ascii_case("Clear") {
        (Rule::Clear, "Clearing")
    } else if !s.eq_ignore_ascii_case("Skip") {
        message(&format!(
            "[{}] <Warning: Unknown Rule \"{s}\"> Worldspace is being skipped.",
            elapsed()
        ));
        (Rule::Skip, "Skipping")
    } else {
        (Rule::Skip, "Skipping")
    };
    message(&format!("[{}] LOD Generator: {action} {}", elapsed(), name(&master)));
    if rule == Rule::Skip {
        return Ok(());
    }
    let mut ref_infos: Vec<RefInfo> = Vec::new();
    let (mut min_cell, mut max_cell) = ((0, 0), (0, 0));
    if rule > Rule::Clear {
        let refrs = find_unique_worldspace_refrs(worldspace);
        let (mut min_x, mut max_x) = (f32::MAX, -f32::MAX);
        let (mut min_y, mut max_y) = (f32::MAX, -f32::MAX);
        for refr in &refrs {
            let Some(base) = refr
                .get_record_by_signature(Signature::new(b"NAME"))
                .and_then(|name| name.get_links_to())
                .and_then(|linked| linked.into_main_record())
            else {
                continue;
            };
            let Some(data) = refr.get_record_by_signature(Signature::new(b"DATA")) else {
                continue;
            };
            let data_count = data.as_container().map_or(0, |c| c.get_element_count());
            if !(has_visible_when_distant_mesh(&base.get_winning_override())
                && data_count == 2
                && !refr.get_form_id().is_null()
                && flags(refr) & FLAG_INITIALLY_DISABLED == 0
                && !refr.get_flags().is_deleted())
            {
                continue;
            }
            let mut info = RefInfo {
                form_id: base.get_load_order_form_id(),
                ..RefInfo::default()
            };
            let data = data.as_container().unwrap();
            let read = |element: Option<ElementRef>| -> Vec<f64> {
                element
                    .map(|e| children(&*e))
                    .unwrap_or_default()
                    .iter()
                    .map(|e| e.get_native_value().as_number().unwrap_or(0.0))
                    .collect()
            };
            let pos = read(data.get_element(0));
            let round = |v: f64| crate::imaging::formats::round(v) as f64;
            if !pos.is_empty() {
                info.pos.0 = (round(pos[0]) + 0.5) as f32;
            }
            if pos.len() >= 2 {
                info.pos.1 = (round(pos[1]) + 0.5) as f32;
            }
            if pos.len() >= 3 {
                info.pos.2 = (round(pos[2]) + 0.970_703_125) as f32;
            }
            let (x, y) = (info.pos.0, info.pos.1);
            if !(-10_000_000.0..=10_000_000.0).contains(&x) || !(-10_000_000.0..=10_000_000.0).contains(&y) {
                message(&format!(
                    "[{}] LOD Generator: <Error while processing {}: Position out of bounds>",
                    elapsed(),
                    name(refr)
                ));
                continue;
            }
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            let rot = read(data.get_element(1));
            let scale = xedit_core::interface::constructors::radians_to_degrees_scale();
            if !rot.is_empty() {
                info.rot.0 = (rot[0] / scale) as f32;
            }
            if rot.len() >= 2 {
                info.rot.1 = (rot[1] / scale) as f32;
            }
            if rot.len() >= 3 {
                info.rot.2 = (rot[2] / scale) as f32;
            }
            info.scale = match refr.get_record_by_signature(Signature::new(b"XSCL")) {
                Some(xscl) => xscl.get_native_value().as_number().unwrap_or(1.0) as f32,
                None => 1.0,
            };
            info.scale = round_to_2(info.scale);
            info.scale = (f64::from(info.scale) * f64::from(lod_scale)) as f32;
            info.scale = (f64::from(info.scale) + f64::from(lod_add)) as f32;
            ref_infos.push(info);
        }
        if ref_infos.is_empty() {
            return Ok(());
        }
        let cell = |value: f32| {
            let mut c = (f64::from(value) / 4096.0).trunc() as i32;
            if value < 0.0 {
                c -= 1;
            }
            c
        };
        min_cell = (cell(min_x), cell(min_y));
        max_cell = (cell(max_x), cell(max_y));
    }
    let lod_path = format!("{}DistantLOD\\", env.output_path);
    force_directories(&lod_path);
    let editor_id = worldspace.get_editor_id();
    // deleting the old .lod files of the worldspace
    if let Ok(entries) = std::fs::read_dir(&lod_path) {
        let prefix = editor_id.to_lowercase();
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().to_string();
            let lower = file.to_lowercase();
            if lower.starts_with(&prefix) && lower.ends_with(".lod") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    if rule > Rule::Clear {
        let mut cmp = Vec::new();
        let width = (max_cell.0 - min_cell.0 + 1) as usize;
        let height = (max_cell.1 - min_cell.1 + 1) as usize;
        // Each cell lists its references last first (the linked list of
        // `Cells`).
        let mut cells: Vec<Vec<usize>> = vec![Vec::new(); width * height];
        for (i, info) in ref_infos.iter().enumerate() {
            let (x, y) = position_to_grid_cell(info.pos);
            let (lx, ly) = ((x - min_cell.0) as usize, (y - min_cell.1) as usize);
            cells[lx * height + ly].insert(0, i);
        }
        for i in 0..width {
            for j in 0..height {
                let list = &cells[i * height + j];
                if list.is_empty() {
                    continue;
                }
                // grouped by base FormID (`wbMergeSortPtr`, stable), each
                // group in the order of the cell's list
                let mut sorted = list.clone();
                sorted.sort_by(|a, b| FormID::compare(ref_infos[*a].form_id, ref_infos[*b].form_id));
                let mut groups: Vec<Vec<usize>> = Vec::new();
                for index in sorted {
                    match groups.last_mut() {
                        Some(group) if ref_infos[group[0]].form_id == ref_infos[index].form_id => {
                            // `RefsInCell[k].Next := RefsInCell[l]`: the new one
                            // goes in front of the group.
                            group.insert(0, index);
                        }
                        _ => groups.push(vec![index]),
                    }
                }
                let mut out = Vec::new();
                out.extend_from_slice(&(groups.len() as u32).to_le_bytes());
                for group in &groups {
                    out.extend_from_slice(&ref_infos[group[0]].form_id.to_cardinal().to_le_bytes());
                    out.extend_from_slice(&(group.len() as u32).to_le_bytes());
                    for &index in group {
                        let p = ref_infos[index].pos;
                        for value in [p.0, p.1, p.2] {
                            out.extend_from_slice(&value.to_le_bytes());
                        }
                    }
                    for &index in group {
                        let r = ref_infos[index].rot;
                        for value in [r.0, r.1, r.2] {
                            out.extend_from_slice(&value.to_le_bytes());
                        }
                    }
                    for &index in group {
                        out.extend_from_slice(&ref_infos[index].scale.to_le_bytes());
                    }
                }
                let x = i as i32 + min_cell.0;
                let y = j as i32 + min_cell.1;
                std::fs::write(format!("{lod_path}{editor_id}_{x}_{y}.lod"), out)?;
                cmp.extend_from_slice(&(y as i16).to_le_bytes());
                cmp.extend_from_slice(&(x as i16).to_le_bytes());
            }
        }
        cmp.extend_from_slice(&7u32.to_le_bytes());
        std::fs::write(format!("{lod_path}{editor_id}.cmp"), cmp)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Skyrim and the Fallouts

/// The settings of a billboard's `.txt`.
fn read_float(ini: &MemIniFile, section: &str, ident: &str, default: f64) -> f64 {
    let text = ini.read_string(section, ident, "");
    xedit_core::delphi::str_to_float(&text).unwrap_or(default)
}

/// `LoadBillboard` of `wbGenerateLODTES5`: a tree of the list with the
/// width and height of its bounds, its billboard texture and the settings
/// of the billboard's `.txt`; index -1 without a billboard.
fn load_billboard(env: &LodEnv, list: &mut TreeList, tree_rec: &MainRecordRef) -> usize {
    let ovr = tree_rec.get_winning_override();
    let (width, height) = if ovr.get_element_exists("OBND") {
        let value = |path: &str| ovr.get_element_native_value(path).as_number().unwrap_or(0.0);
        (
            (value("OBND\\X2") - value("OBND\\X1")) as f32,
            (value("OBND\\Z2") - value("OBND\\Z1")) as f32,
        )
    } else {
        (0.0, 0.0)
    };
    let file_name = tree_rec.get_file().map(|file| file.get_name()).unwrap_or_default();
    let index = list.add_tree(
        &file_name,
        &ovr.get_element_edit_value("Model\\MODL"),
        tree_rec.get_load_order_form_id(),
        width,
        height,
    );
    let billboard = list.trees[index].billboard.clone();
    let loaded = open_resource(&billboard)
        .filter(|data| load_tree_image(env, &mut list.trees[index], data))
        .map(|data| list.trees[index].crc32 = xedit_io::hash::crc32(&data) as i32);
    if loaded.is_some() {
        if let Some(data) = open_resource(&change_file_ext(&billboard, ".txt")) {
            let ini = MemIniFile::from_text(&xedit_io::encoding::string_list_text(&data));
            let tree = &mut list.trees[index];
            // don't read Width and Height from ini if they are 0
            if !crate::nif_math::same_value(read_float(&ini, "LOD", "Width", 0.0), 0.0) {
                tree.width = read_float(&ini, "LOD", "Width", f64::from(tree.width)) as f32;
            }
            if !crate::nif_math::same_value(read_float(&ini, "LOD", "Height", 0.0), 0.0) {
                tree.height = read_float(&ini, "LOD", "Height", f64::from(tree.height)) as f32;
            }
            tree.shift_x = read_float(&ini, "LOD", "ShiftX", 0.0) as f32;
            tree.shift_y = read_float(&ini, "LOD", "ShiftY", 0.0) as f32;
            tree.shift_z = read_float(&ini, "LOD", "ShiftZ", 0.0) as f32;
            tree.scale_factor = read_float(&ini, "LOD", "Scale", 1.0) as f32;
        }
    } else {
        list.trees[index].index = -1;
    }
    index
}

/// The LOD settings of the worldspace, or the message that there are none.
fn lod_settings(worldspace: &MainRecordRef) -> LodResult<Option<LodSettings>> {
    match open_resource(&lod_settings_file_name(&worldspace.get_editor_id())) {
        Some(data) => Ok(Some(LodSettings::load_from_data(&data)?)),
        None => {
            message(&format!(
                "[{}] Lodsettings file not found for worldspace.",
                worldspace.get_editor_id()
            ));
            Ok(None)
        }
    }
}

/// `GetLargeReferencesPlugin`: the references the `RNAM` of an ESM's
/// worldspace lists in the grid cell they are in (in a chunk when it is
/// set).
fn large_references_plugin(element: Option<ElementRef>, list: &mut StringList, chunk: ((i32, i32), (i32, i32))) {
    let Some(element) = element else {
        return;
    };
    // RNAM data working in ESM only for now
    if !element.get_file().is_some_and(|file| file.get_is_esm()) {
        return;
    }
    let (sw, ne) = chunk;
    for grid_entry in children(&*element) {
        let Some(grid) = grid_entry.as_container() else {
            continue;
        };
        let value = |path: &str| {
            grid.get_element_by_path(path)
                .map_or(0, |e| e.get_native_value().as_ordinal().unwrap_or(0) as i32)
        };
        let (gx, gy) = (value("X"), value("Y"));
        if sw.0 != i32::MIN && (gx < sw.0 || gx > ne.0) {
            continue;
        }
        if sw.1 != i32::MIN && (gy < sw.1 || gy > ne.1) {
            continue;
        }
        let Some(references) = grid.get_element_by_path("References") else {
            continue;
        };
        for reference_entry in children(&*references) {
            let Some(entry) = reference_entry.as_container() else {
                continue;
            };
            let Some(reference) = entry
                .get_element_by_path("Ref")
                .and_then(|r| r.get_links_to())
                .and_then(|r| r.into_main_record())
            else {
                continue;
            };
            let pos = get_position(&reference).unwrap_or_default();
            let cell = position_to_grid_cell(pos);
            if (gx, gy) == cell {
                let master = reference.get_master_or_self();
                list.add_object(
                    &master.get_load_order_form_id().to_string(false),
                    master.get_element_id() as u64,
                );
            }
        }
    }
}

/// The parts of a tab separated line with one part changed.
fn with_part(s: &str, index: usize, change: impl Fn(&str) -> String) -> String {
    let parts = delimited_text(s, '\t');
    let mut out = String::new();
    for (j, part) in parts.iter().enumerate() {
        if j != index {
            out.push_str(part);
        } else {
            out.push_str(&change(part));
        }
        if j + 1 < parts.len() {
            out.push('\t');
        }
    }
    out
}

/// The options of the objects LOD read from the settings: the chunk and
/// area limits.
struct ChunkOptions {
    chunk: bool,
    area: bool,
    build_atlas: bool,
    sw: (i32, i32),
    ne: (i32, i32),
}

fn str_to_int_def(text: &str, default: i32) -> i32 {
    crate::variant::str_to_int(text).unwrap_or(default)
}

/// `wbGenerateLODTES5`: the tree LOD (unless trees are 3D objects) and the
/// objects LOD of a worldspace of Skyrim or the Fallouts.
pub fn generate_lod_tes5(
    env: &LodEnv,
    worldspace: &MainRecordRef,
    lod_trees: bool,
    lod_objects: bool,
    files: &[Arc<FileImpl>],
) -> LodResult<()> {
    let master = worldspace.get_master_or_self();
    let Some(lod_set) = lod_settings(worldspace)? else {
        return Ok(());
    };
    let section = env.section();
    let trees_3d = env.read_bool(&section, "Trees3D", true);
    let refrs = find_unique_worldspace_refrs(worldspace);
    if refrs.is_empty() {
        return Ok(());
    }
    let id = worldspace.get_editor_id();
    message(&format!("[{id}] Generating LOD"));

    if lod_trees && !trees_3d {
        let mut log = StringList::new();
        let lod_level = if is_fallout3() { 8 } else { 4 };
        let mut list = TreeList::new(&id);
        let mut lod4: Vec<TreeBlock> = Vec::new();
        let result = (|| -> LodResult<()> {
            let (mut trees_count, mut trees_dup_count) = (0, 0);
            // Fallouts use common atlas for all worldspaces, so all available
            // billboards are collected
            if is_fallout3() {
                for file in files {
                    for signature in [b"STAT", b"ACTI", b"TREE"] {
                        let Some(group) = file.group_by_signature(Signature::new(signature)) else {
                            continue;
                        };
                        for element in children(&*group) {
                            let Some(tree_rec) = element.into_main_record() else {
                                continue;
                            };
                            if !tree_rec.get_is_master() || flags(&tree_rec.get_winning_override()) & 0x40 == 0 {
                                continue;
                            }
                            let index = load_billboard(env, &mut list, &tree_rec);
                            add_billboard_log(&mut log, &list, index, &tree_rec);
                        }
                    }
                }
            }
            for refr in &refrs {
                let Some(base) = refr.get_base_record() else {
                    continue;
                };
                let tree_rec = base.get_master_or_self();
                let signature = tree_rec.get_signature();
                if is_skyrim() {
                    if signature != Signature::new(b"TREE") && signature != Signature::new(b"STAT") {
                        continue;
                    }
                    if signature == Signature::new(b"STAT") && flags(&tree_rec) & 0x40 == 0 {
                        continue;
                    }
                }
                if is_fallout3() && list.tree_by_form_id(tree_rec.get_load_order_form_id()).is_none() {
                    continue;
                }
                let Some(mut ref_pos) = get_position(refr) else {
                    continue;
                };
                let ref_cell = position_to_grid_cell(ref_pos);
                let ref_block = lod_set.block_for_cell(ref_cell, lod_level);
                if ref_block.0 < lod_set.sw_cell.0 || ref_block.1 < lod_set.sw_cell.1 {
                    continue;
                }
                // find existing block or add a new one, for empty blocks too
                let k = match lod4.iter().position(|block| block.cell == ref_block) {
                    Some(k) => k,
                    None => {
                        lod4.push(TreeBlock::new(ref_block, lod_level));
                        lod4.len() - 1
                    }
                };
                // skip invisible references
                if flags(refr) & FLAG_INITIALLY_DISABLED != 0
                    || refr.get_flags().is_deleted()
                    || refr.get_element_exists("XESP")
                {
                    continue;
                }
                // Skyrim: skip persistent "Is Full LOD" tree refs
                if is_skyrim() && refr.get_is_persistent() && flags(refr) & 0x0001_0000 != 0 {
                    continue;
                }
                let tree = match list.tree_by_form_id(tree_rec.get_load_order_form_id()) {
                    Some(tree) => tree,
                    None => {
                        let index = load_billboard(env, &mut list, &tree_rec);
                        add_billboard_log(&mut log, &list, index, &tree_rec);
                        index
                    }
                };
                if list.trees[tree].index == -1 {
                    continue;
                }
                // Trees LOD can't be rotated around x and y, reject "fallen" trees
                if let Some(rot) = get_rotation(refr)
                    && ((rot.0 > 30.0 && rot.0 < 330.0) || (rot.1 > 30.0 && rot.1 < 330.0))
                {
                    continue;
                }
                let t = &list.trees[tree];
                ref_pos.0 = (f64::from(ref_pos.0) + f64::from(t.shift_x)) as f32;
                ref_pos.1 = (f64::from(ref_pos.1) + f64::from(t.shift_y)) as f32;
                ref_pos.2 = (f64::from(ref_pos.2) + f64::from(t.shift_z)) as f32;
                let mut scale: f32 = if refr.get_element_exists("XSCL") {
                    refr.get_element_native_value("XSCL").as_number().unwrap_or(0.0) as f32
                } else {
                    1.0
                };
                scale = (f64::from(scale) * f64::from(t.scale_factor)) as f32;
                let tree_index = t.index;
                let ref_form_id = if is_skyrim() {
                    refr.get_load_order_form_id()
                } else if refr.get_is_master() {
                    refr.get_fixed_form_id()
                } else {
                    refr.get_fixed_form_id().change_file_id(FileID::create_full(1))
                };
                if lod4[k].add_reference(&mut list, ref_form_id, tree_index, ref_pos, scale) {
                    trees_count += 1;
                } else {
                    trees_dup_count += 1;
                }
            }
            log.sort();
            message(trim(&log.text()));
            let max_size = str_to_int_def(&env.read_string("Worldspace", "AtlasSizeMax", ""), 8192);
            list.build_atlas(max_size)?;
            if list.trees_list.is_empty() || (is_fallout3() && trees_count == 0) {
                message(&format!(
                    "<Note: Can not build Trees LOD for {id}, no resource billboards or valid tree references found>"
                ));
            } else {
                let lod_path = env.output_path.clone();
                let folder = format!("{lod_path}{}", extract_file_path(&list.atlas_file_name()));
                if let Ok(entries) = std::fs::read_dir(&folder) {
                    let extension = format!(".{}", lod_tree_block_file_ext());
                    for entry in entries.flatten() {
                        if entry.file_name().to_string_lossy().to_lowercase().ends_with(&extension) {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
                if !lod4.is_empty() {
                    list.change_atlas_brightness(env.read_integer(&section, "TreesBrightness", 0))?;
                    force_directories(&format!("{lod_path}{}", extract_file_path(&list.atlas_file_name())));
                    list.save_atlas(&format!("{lod_path}{}", list.atlas_file_name()))
                        .map_err(|_| LodError::new("Can't save atlas"))?;
                    force_directories(&format!("{lod_path}{}", extract_file_path(&list.list_file_name())));
                    list.save_to_file(&format!("{lod_path}{}", list.list_file_name()))?;
                    for block in &lod4 {
                        block.save_to_file(&format!("{lod_path}{}", block.file_name(&list)))?;
                    }
                }
                message(&format!("[{id}] Trees LOD Done."));
                if trees_dup_count != 0 {
                    message(&format!(
                        "<Warning: {trees_dup_count} duplicate FormID numbers of trees references were detected, excluded from LOD>"
                    ));
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            message(&format!("[{id}] Trees LOD generation error: {error}"));
        }
    }

    if lod_objects {
        let result = objects_lod_tes5(env, worldspace, &master, &refrs, &lod_set, trees_3d);
        if let Err(error) = result {
            message(&format!("[{id}] Objects LOD generation error: {error}"));
        }
    }
    Ok(())
}

/// The log line of a billboard: the texture it uses or that it has none.
fn add_billboard_log(log: &mut StringList, list: &TreeList, index: usize, tree_rec: &MainRecordRef) {
    let tree = &list.trees[index];
    if tree.index != -1 {
        log.add(&format!("{} using LOD {}", name(tree_rec), tree.billboard));
    } else {
        log.add(&format!("<Note: {} LOD not found {}>", name(tree_rec), tree.billboard));
    }
}

/// The objects LOD of `wbGenerateLODTES5`.
fn objects_lod_tes5(
    env: &LodEnv,
    worldspace: &MainRecordRef,
    master: &MainRecordRef,
    refrs: &[MainRecordRef],
    lod_set: &LodSettings,
    trees_3d: bool,
) -> LodResult<()> {
    let section = env.section();
    let id = worldspace.get_editor_id();
    let mut cache = StringList::new();
    let mut cache_hp_lod = StringList::new();
    let mut refs = StringList::new();
    let mut export = StringList::new();
    let mut lod_meshes = StringList::sorted();
    let mut lod_textures = StringList::sorted();
    let mut large_references = StringList::sorted();
    let mut list = TreeList::new(&id);
    let chunk = env.read_bool(&section, "Chunk", false);
    // the chunk option works as an area limiter if upper boundaries are set
    let area = !env.read_string(&section, "LODX2", "").is_empty() && !env.read_string(&section, "LODY2", "").is_empty();
    let build_atlas = env.read_bool(&section, "BuildAtlas", true);
    let mut chunk_sw = (i32::MIN, i32::MIN);
    let mut chunk_ne = (0, 0);
    let mut chunk_size = if is_fallout4() { 32 } else { 16 };
    if chunk {
        chunk_size = str_to_int_def(&env.read_string(&section, "LODLevel", ""), chunk_size);
        chunk_sw.0 = str_to_int_def(&env.read_string(&section, "LODX", &i32::MIN.to_string()), i32::MIN);
        chunk_sw.1 = str_to_int_def(&env.read_string(&section, "LODY", &i32::MIN.to_string()), i32::MIN);
        chunk_ne = (chunk_sw.0.wrapping_add(chunk_size), chunk_sw.1.wrapping_add(chunk_size));
    }
    if area {
        chunk_ne.0 = str_to_int_def(&env.read_string(&section, "LODX2", &i32::MAX.to_string()), i32::MAX);
        chunk_ne.1 = str_to_int_def(&env.read_string(&section, "LODY2", &i32::MAX.to_string()), i32::MAX);
    }
    let options = ChunkOptions {
        chunk,
        area,
        build_atlas,
        sw: chunk_sw,
        ne: chunk_ne,
    };
    // gather large references if LOD level 4 is generated
    if matches!(game_mode(), GameMode::gmSSE | GameMode::gmTES5VR) {
        let level = env.read_string(&section, "LODLevel", "");
        if (level.is_empty() || level == "4")
            && let Some(master) = record_impl(master)
        {
            {
                large_references_plugin(
                    master.get_element_by_path("RNAM"),
                    &mut large_references,
                    (chunk_sw, chunk_ne),
                );
                for over in master.overrides() {
                    large_references_plugin(
                        over.get_element_by_path("RNAM"),
                        &mut large_references,
                        (chunk_sw, chunk_ne),
                    );
                }
            }
        }
    }
    // `mat` is a variable of the routine, which keeps its value from one
    // static to the next.
    let mut mat = String::new();
    for refr in refrs {
        let Some(stat_rec) = refr.get_base_record() else {
            continue;
        };
        let signature = stat_rec.get_signature();
        let sig = |s: &[u8; 4]| signature == Signature::new(s);
        if is_skyrim() && !sig(b"STAT") && !sig(b"TREE") {
            continue;
        }
        if is_fallout3() && !sig(b"STAT") && !sig(b"SCOL") && !sig(b"ACTI") && !sig(b"MSTT") {
            continue;
        }
        if flags(refr) & FLAG_INITIALLY_DISABLED != 0 || refr.get_flags().is_deleted() {
            continue;
        }
        // Skip parent enabled refs which have XESP element, unless VWD
        if !refr.get_flags().is_visible_when_distant() {
            let xesp = refr.get_element_by_path("XESP\\Reference");
            if xesp.is_some() && !is_fallout3() {
                continue;
            }
            // Fallout 3 Megaton refs use the 'apocalypse' LOD when destroyed
            if let Some(xesp) = xesp
                && is_fallout3()
                && let Some(link) = xesp.get_links_to().and_then(|l| l.into_main_record())
            {
                // skip ordinary enabled refs, and the toggle's refs that are opposite enabled
                if link.get_editor_id() != "MS11MegatonToggle"
                    || refr.get_element_native_value("XESP\\Flags").as_ordinal().unwrap_or(0) & 1 == 1
                {
                    continue;
                }
            }
        }
        let stat_rec = stat_rec.get_winning_override();
        // Skyrim: skip persistent refs of "never fade" statics and "Is Full LOD" refs
        if is_skyrim() && refr.get_is_persistent() && (flags(&stat_rec) & 0x4 != 0 || flags(refr) & 0x0001_0000 != 0) {
            continue;
        }
        let Some(ref_pos) = get_position(refr) else {
            continue;
        };
        let ref_cell = position_to_grid_cell(ref_pos);
        let ref_block = lod_set.block_for_cell(ref_cell, 4);
        if ref_block.0 < lod_set.sw_cell.0 || ref_block.1 < lod_set.sw_cell.1 {
            continue;
        }
        // reference not in specific chunk: skipped only if no atlas needs to be built, or in an area
        if options.area || (options.chunk && !options.build_atlas) {
            if options.sw.0 != i32::MIN && (ref_cell.0 < options.sw.0 || ref_cell.0 > options.ne.0) {
                continue;
            }
            if options.sw.1 != i32::MIN && (ref_cell.1 < options.sw.1 || ref_cell.1 > options.ne.1) {
                continue;
            }
        }
        let scl = if refr.get_element_exists("XSCL") {
            refr.get_element_edit_value("XSCL")
        } else {
            "1.0".to_owned()
        };
        let key = u64::from(stat_rec.get_load_order_form_id().to_cardinal());
        let k = match cache.index_of_object(key) {
            Some(k) => k,
            None => {
                let mut s = String::new();
                let stat_sig = stat_rec.get_signature();
                if (is_skyrim()
                    && (stat_sig == Signature::new(b"TREE") || stat_rec.get_flags().is_visible_when_distant()))
                    || is_fallout3()
                {
                    let mut meshes = [
                        get_lod_mesh_name(&stat_rec, 0, trees_3d),
                        get_lod_mesh_name(&stat_rec, 1, trees_3d),
                        get_lod_mesh_name(&stat_rec, 2, trees_3d),
                    ];
                    for (level, lod) in [(0usize, "LOD4"), (1, "LOD8")] {
                        // notify about 3D tree mesh or fallback to billboard
                        if trees_3d && stat_sig == Signature::new(b"TREE") {
                            if !meshes[level].is_empty() {
                                message(&format!("{} using 3D mesh in {lod} {}", name(&stat_rec), meshes[level]));
                            } else {
                                let tree = match list.tree_by_form_id(stat_rec.get_load_order_form_id()) {
                                    Some(tree) => tree,
                                    None => load_billboard(env, &mut list, &stat_rec),
                                };
                                if list.trees[tree].index != -1 {
                                    meshes[level] = list.trees[tree].billboard.clone();
                                    message(&format!(
                                        "{} using 3D flat billboard in {lod} {}",
                                        name(&stat_rec),
                                        meshes[level]
                                    ));
                                } else {
                                    message(&format!(
                                        "<Note: {} {lod} not found {}>",
                                        name(&stat_rec),
                                        list.trees[tree].billboard
                                    ));
                                }
                            }
                        }
                        if !meshes[level].is_empty() {
                            lod_meshes.add(&meshes[level]);
                        }
                    }
                    // don't fallback to billboards in LOD16 since it is used for the map
                    if trees_3d && stat_sig == Signature::new(b"TREE") && !meshes[2].is_empty() {
                        message(&format!("{} using 3D mesh in LOD16 {}", name(&stat_rec), meshes[2]));
                    }
                    if !meshes[2].is_empty() {
                        lod_meshes.add(&meshes[2]);
                    }
                    let [m4, m8, m16] = &meshes;
                    if !m4.is_empty() || !m8.is_empty() || !m16.is_empty() {
                        // detecting LOD material
                        mat = String::new();
                        if is_skyrim()
                            && stat_rec.get_element_exists("DNAM\\Material")
                            && let Some(ovr) = stat_rec
                                .get_element_by_path("DNAM\\Material")
                                .and_then(|m| m.get_links_to())
                                .and_then(|m| m.into_main_record())
                        {
                            let lower = ovr.get_editor_id().to_lowercase();
                            mat = lower.clone();
                            if lower.contains("snow") {
                                mat = "Snow".to_owned();
                            } else if lower.contains("ash") {
                                mat = "Ash".to_owned();
                            } else if lower.contains("passthru") {
                                mat = "PassThru".to_owned();
                            }
                        }
                        s = format!(
                            "{}\t{}\t{mat}\t{}\t{m4}\t{m8}\t{m16}",
                            stat_rec.get_editor_id(),
                            hex8(flags(&stat_rec)),
                            get_lod_mesh_name(&stat_rec, -1, false)
                        );
                    }
                    let k = cache.len();
                    cache.add_object(&s, key);
                    // Fallouts: High Priority LOD info with m4 model for m8
                    if is_fallout3() {
                        let hp = if s.is_empty() {
                            s.clone()
                        } else {
                            format!(
                                "{}\t{}\t{mat}\t{}\t{m4}\t{m4}\t{m16}",
                                stat_rec.get_editor_id(),
                                hex8(flags(&stat_rec)),
                                get_lod_mesh_name(&stat_rec, -1, false)
                            )
                        };
                        cache_hp_lod.add(&hp);
                    }
                    k
                } else {
                    let k = cache.len();
                    cache.add_object(&s, key);
                    if is_fallout3() {
                        cache_hp_lod.add(&s);
                    }
                    k
                }
            }
        };
        if cache.get(k).is_empty() {
            continue;
        }
        // Fallouts: High Priority LOD references info from separate cache
        let mut s = if is_fallout3() && flags(refr) & 0x0001_0000 != 0 {
            cache_hp_lod.get(k).to_owned()
        } else {
            cache.get(k).to_owned()
        };
        // SSE adds -LargeRef to the material for LODGen.exe
        if matches!(game_mode(), GameMode::gmSSE | GameMode::gmTES5VR)
            && large_references
                .index_of_object(refr.get_master_or_self().get_element_id() as u64)
                .is_some()
        {
            s = with_part(&s, 2, |part| format!("{part}-LargeRef"));
        }
        let pos = |axis: &str| refr.get_element_edit_value(&format!("DATA\\Position\\{axis}"));
        let rot = |axis: &str| refr.get_element_edit_value(&format!("DATA\\Rotation\\{axis}"));
        refs.add(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{scl}\t{s}",
            refr.get_load_order_form_id().to_string(false),
            hex8(flags(refr)),
            pos("X"),
            pos("Y"),
            pos("Z"),
            rot("X"),
            rot("Y"),
            rot("Z")
        ));
    }
    if refs.is_empty() {
        message(&format!(
            "<Note: Can not build Objects LOD for {id}, no valid references found>"
        ));
        return Ok(());
    }
    let mut uv_range: f32 = 0.0;
    let mut atlas_name = String::new();
    let mut atlas_map_name = String::new();
    if build_atlas {
        uv_range = xedit_core::delphi::str_to_float(&env.read_string(&section, "AtlasTextureUVRange", "1.5"))
            .unwrap_or(1.5) as f32;
        if is_skyrim() {
            atlas_name = format!(
                "{}textures\\terrain\\{id}\\Objects\\{id}ObjectsLOD.dds",
                env.output_path
            );
        } else if is_fallout3() {
            atlas_name = format!(
                "{}textures\\landscape\\lod\\{id}\\Blocks\\{id}.Buildings.dds",
                env.output_path
            );
        }
        atlas_map_name = format!("{}LODGenAtlasMap.txt", env.scripts_path);
        let folder = extract_file_path(&atlas_name);
        if !std::path::Path::new(folder).is_dir() && !force_directories(folder) {
            return Err(LodError::new(format!(
                "Can not create output folder for atlas {folder}"
            )));
        }
    } else if is_skyrim() {
        // use vanilla atlas if build atlas is not selected
        atlas_map_name = format!("{}{}-AtlasMap-{id}.txt", env.scripts_path, app_name());
        uv_range = 10000.0;
    }
    export.add(&format!("GameMode={}", app_name()));
    export.add(&format!("Worldspace={id}"));
    export.add(&format!("CellSW={} {}", lod_set.sw_cell.0, lod_set.sw_cell.1));
    let winning = worldspace.get_winning_override();
    if is_skyrim() {
        export.add(&format!("TextureDiffuseHD={}", winning.get_element_edit_value("TNAM")));
        export.add(&format!("TextureNormalHD={}", winning.get_element_edit_value("UNAM")));
    }
    if is_fallout3() && lod_set.object_level == 8 {
        export.add("Level8=True");
    }
    if !atlas_map_name.is_empty() {
        export.add(&format!("TextureAtlasMap={atlas_map_name}"));
        export.add(&format!(
            "AtlasTolerance={}",
            float_to_str_f_fixed(f64::from(uv_range) - 1.0, 1)
        ));
    }
    export.add(&format!("PathData={}", env.data_path));
    if is_skyrim() {
        export.add(&format!("PathOutput={}meshes\\terrain\\{id}\\Objects", env.output_path));
    } else if is_fallout3() {
        export.add(&format!(
            "PathOutput={}meshes\\landscape\\lod\\{id}\\Blocks",
            env.output_path
        ));
    } else {
        return Err(LodError::new("Unsupported LODGen game"));
    }
    resource_lines(&mut export);
    // adding list of meshes to ignore translation/rotation
    let ignore = if is_skyrim() {
        env.read_string(&section, "IgnoreTranslation", MESH_IGNORE_TRANSLATION_TES5)
    } else {
        env.read_string(&section, "IgnoreTranslation", "nif")
    };
    for mesh in delimited_text(&ignore, ',') {
        export.add(&format!("IgnoreTranslation={mesh}"));
    }
    // billboards list for 3D trees LOD
    if trees_3d {
        let mut flat = StringList::new();
        for tree in &list.trees {
            if tree.index != -1 {
                flat.add(&format!(
                    "{}\t{}\t{}\t{}\t{}\t1\t{}LODGen_flat_lod.nif\t\t-1\t-1",
                    tree.billboard,
                    float_to_str(f64::from(tree.width)),
                    float_to_str(f64::from(tree.height)),
                    float_to_str(f64::from(tree.shift_z)),
                    float_to_str(f64::from(tree.scale_factor)),
                    env.scripts_path
                ));
            }
        }
        let s = format!("{}LODGenFlatTextures.txt", env.scripts_path);
        flat.save_to_file(&s)?;
        export.add(&format!("FlatTextures={s}"));
    }
    extra_options(env, worldspace, &mut export);
    for line in refs.strings() {
        export.add(line);
    }
    let export_name = format!("{}LODGen.txt", env.scripts_path);
    message(&format!("[{id}] Saving LODGen data: {export_name}"));
    export.save_to_file(&export_name)?;
    if build_atlas {
        // Fallout 3 and FNV don't support several shapes in LOD quads
        if is_fallout3() {
            uv_range = 10000.0;
        }
        get_uv_range_textures_list(&lod_meshes, &mut lod_textures, uv_range)?;
        if lod_textures.len() > 1 {
            // remove HD LOD texture if there
            if is_skyrim() {
                let hd = get_asset_name(&winning.get_element_edit_value("TNAM"), "", AssetType::Texture);
                if let Some(i) = lod_textures.index_of(&hd) {
                    lod_textures.delete(i);
                }
            }
            message(&format!("[{id}] Building LOD textures atlas: {atlas_name}"));
            let size = env.read_integer(&section, "AtlasTextureSize", 512);
            build_atlas_from_textures_list(
                env,
                &lod_textures,
                size,
                size,
                env.read_integer(&section, "AtlasWidth", 2048),
                env.read_integer(&section, "AtlasHeight", 2048),
                &atlas_name,
                &atlas_map_name,
            )?;
        }
    }
    let mut command = format!("\"{}{LODGEN_NAME}\" \"{export_name}\"", env.scripts_path);
    command.push_str(" --dontFixTangents");
    command.push_str(" --removeUnseenFaces");
    // if "No LOD Water" flag is set for a worldspace, then don't remove underwater meshes
    let data_flags = winning.get_element_native_value("DATA").as_ordinal().unwrap_or(0);
    if (is_skyrim() && data_flags & 0x08 != 0) || (is_fallout3() && data_flags & 0x10 != 0) {
        command.push_str(" --ignoreWater");
    }
    if env.read_bool(&section, "ObjectsNoVertexColors", false) {
        command.push_str(" --dontGenerateVertexColors");
    }
    if env.read_bool(&section, "ObjectsNoTangents", false) {
        command.push_str(" --dontGenerateTangents");
    }
    if chunk && !area {
        for (option, ident) in [("--lodLevel", "LODLevel"), ("--x", "LODX"), ("--y", "LODY")] {
            let value = env.read_string(&section, ident, "");
            if !value.is_empty() {
                command.push_str(&format!(" {option} {value}"));
            }
        }
    }
    message(&format!("[{id}] Running {command}"));
    run_lodgen(&command)?;
    // disable traditional Trees LOD if trees are generated as objects
    if trees_3d {
        let s = format!("{}{}", env.data_path, list.list_file_name());
        if std::path::Path::new(&s).is_file() {
            let _ = std::fs::remove_file(&s);
        }
        if resource_exists(&list.list_file_name()) {
            force_directories(extract_file_path(&s));
            std::fs::write(&s, [0u8; 4])?;
            message(&format!(
                "<Note: Trees LOD list file exists in archives, creating an empty loose one to disable native LOD: {}>",
                list.list_file_name()
            ));
        }
    }
    message(&format!("[{id}] Objects LOD Done."));
    // DynDOLOD reference message, tribute to Sheson who made TES5LODGen possible
    if is_skyrim() {
        message(&"*".repeat(120));
        message("If you want more detailed, dynamic LOD with wide customization, please check DynDOLOD by Sheson");
        message("http://www.nexusmods.com/skyrim/mods/59721/");
        message("It uses the same LODGen building process as TES5LODGen internally, but with more options.");
        message(&"*".repeat(120));
        message("");
    }
    Ok(())
}

/// `sMeshIgnoreTranslationTES5`.
const MESH_IGNORE_TRANSLATION_TES5: &str = "meshes\\lod\\solitude\\cwtower01_lod.nif,\
meshes\\lod\\solitude\\sfarmhousesilo_lod.nif,\
meshes\\lod\\solitude\\slumbermill01_lod.nif,\
meshes\\lod\\solitude\\spatiowall02_lod.nif,\
meshes\\lod\\solitude\\spatiowall03_lod.nif,\
meshes\\lod\\solitude\\spatiowall30_lod.nif,\
meshes\\lod\\solitude\\spatiowallsteps01_lod.nif,\
meshes\\lod\\solitude\\spatiowallsteps02_lod.nif,\
meshes\\lod\\solitude\\sstyrrshouse_lod.nif,\
meshes\\lod\\solitude\\sthe winking skeever_lod.nif,\
meshes\\lod\\windhelm\\wharena_lod.nif,\
meshes\\lod\\windhelm\\whbrunwulfsq_lod.nif,\
meshes\\lod\\windhelm\\whgrayquarter_lod.nif,\
meshes\\lod\\windhelm\\whinnerwall01_lod.nif,\
meshes\\lod\\windhelm\\whinnerwall02_lod.nif,\
meshes\\lod\\windhelm\\whinnland_lod.nif,\
meshes\\lod\\windhelm\\whmaingate_lod.nif,\
meshes\\lod\\windhelm\\whmarket01_lod.nif,\
meshes\\lod\\windhelm\\whouterwall3_lod.nif,\
meshes\\lod\\windhelm\\whpalace_lod.nif,\
meshes\\lod\\windhelm\\whtempletalos_lod.nif,\
meshes\\lod\\windhelm\\whvalunstrad_lod.nif";

// ---------------------------------------------------------------------------
// Fallout 4

/// What `ProcessReference` of `wbGenerateLODFO4` collects.
struct Fo4State {
    lod_set: LodSettings,
    chunk: bool,
    build_atlas: bool,
    chunk_sw: (i32, i32),
    chunk_ne: (i32, i32),
    cache: StringList,
    refs: StringList,
    lod_meshes: StringList,
    bgsm: StringList,
}

/// `TStrings.DelimitedText` (the getter) with a delimiter that is not
/// strict: a field with a character up to the space, the delimiter or the
/// quote is quoted, a single empty field is `""`.
fn delimited_text_getter(fields: &[String], delimiter: char) -> String {
    if fields.len() == 1 && fields[0].is_empty() {
        return "\"\"".to_owned();
    }
    let mut text = String::new();
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            text.push(delimiter);
        }
        let stop = field
            .chars()
            .take_while(|&c| !(c == '\0' || c == '"' || c == delimiter || ('\u{1}'..=' ').contains(&c)));
        if stop.count() < field.chars().count() && !field.starts_with('\0') {
            text.push('"');
            text.push_str(&field.replace('"', "\"\""));
            text.push('"');
        } else {
            text.push_str(field);
        }
    }
    text
}

impl Fo4State {
    /// `ProcessReference`: a reference, one part and one placement of a
    /// static collection at a time.
    fn process_reference(&mut self, e: &MainRecordRef, i_part: i32, i_placement: i32, recursive: bool) {
        if e.get_flags().is_deleted() {
            return;
        }
        let Some(stat_rec) = e.get_base_record() else {
            return;
        };
        let mut stat_rec = stat_rec.get_winning_override();
        let sig = stat_rec.get_signature();
        if sig != Signature::new(b"STAT") && sig != Signature::new(b"SCOL") {
            return;
        }
        let Some(ref_pos) = get_position(e) else {
            return;
        };
        // skip markers, markers with VWD are MultiRef LODs and need to be kept
        if e.get_is_persistent() && !e.get_flags().is_visible_when_distant() && flags(&stat_rec) & 0x4 != 0 {
            return;
        }
        // skip initially disabled without XESP and without VWD
        if flags(e) & FLAG_INITIALLY_DISABLED != 0
            && !e.get_element_exists("XESP")
            && !e.get_flags().is_visible_when_distant()
        {
            return;
        }
        let mut xespid = String::new();
        if e.get_element_exists("XESP") {
            let xesp = e
                .get_element_links_to("XESP\\Reference")
                .and_then(|x| x.into_main_record());
            match xesp {
                Some(xesp) if flags(&xesp) & 0x100 == 0x100 => xespid = xesp.get_form_id().to_string(false),
                Some(xesp) if !e.get_flags().is_visible_when_distant() => {
                    let mut initially_disabled = flags(&xesp) & FLAG_INITIALLY_DISABLED != 0;
                    if e.get_element_native_value("XESP\\Flags").as_ordinal().unwrap_or(0) & 1 == 1 {
                        initially_disabled = !initially_disabled;
                    }
                    if initially_disabled {
                        return;
                    }
                }
                _ => {}
            }
        }
        let ref_cell = position_to_grid_cell(ref_pos);
        let ref_block = self.lod_set.block_for_cell(ref_cell, 4);
        if ref_block.0 < self.lod_set.sw_cell.0 || ref_block.1 < self.lod_set.sw_cell.1 {
            return;
        }
        if self.chunk && !self.build_atlas {
            if self.chunk_sw.0 != i32::MIN && (ref_cell.0 < self.chunk_sw.0 || ref_cell.0 > self.chunk_ne.0) {
                return;
            }
            if self.chunk_sw.1 != i32::MIN && (ref_cell.1 < self.chunk_sw.1 || ref_cell.1 > self.chunk_ne.1) {
                return;
            }
        }
        // SCOL static collection: the transformation of a part
        let mut transrot = "\t\t\t\t\t\t".to_owned();
        let mut max_placement = -1;
        if stat_rec.get_signature() == Signature::new(b"SCOL")
            && stat_rec.get_element_exists("Parts")
            && let Some(parts) = stat_rec.get_element_by_name("Parts")
            && let Some(parts) = parts.as_container()
        {
            let Some(entry) = parts.get_element(i_part) else {
                return;
            };
            // process the next part
            if i_placement == 0 && i_part < parts.get_element_count() - 1 {
                self.process_reference(e, i_part + 1, i_placement, false);
            }
            let Some(entry) = entry.as_container() else {
                return;
            };
            let Some(part_stat) = entry
                .get_element_links_to("ONAM - Static")
                .and_then(|s| s.into_main_record())
            else {
                return;
            };
            stat_rec = part_stat.get_winning_override();
            if entry.get_element_exists("DATA - Placements")
                && let Some(placements) = entry.get_element_by_name("DATA - Placements")
                && let Some(placements) = placements.as_container()
            {
                max_placement = placements.get_element_count() - 1;
                let Some(placement) = placements.get_element(i_placement) else {
                    return;
                };
                let Some(placement) = placement.as_container() else {
                    return;
                };
                let value = |path: &str| {
                    placement
                        .get_element_by_path(path)
                        .map_or_else(String::new, |v| v.get_edit_value())
                };
                transrot = [
                    value("Position\\X"),
                    value("Position\\Y"),
                    value("Position\\Z"),
                    value("Rotation\\X"),
                    value("Rotation\\Y"),
                    value("Rotation\\Z"),
                    value("Scale"),
                ]
                .join("\t");
            }
        }
        let key = u64::from(stat_rec.get_load_order_form_id().to_cardinal());
        // `m4` to `m32` keep their values only for a static seen the first
        // time (they are '' for a cached one).
        let (mut m4, mut m8, mut m16, mut m32) = (String::new(), String::new(), String::new(), String::new());
        let k = match self.cache.index_of_object(key) {
            Some(k) => k,
            None => {
                let mut s = String::new();
                if stat_rec.get_flags().is_visible_when_distant() {
                    m4 = get_lod_mesh_name(&stat_rec, 0, false);
                    m8 = get_lod_mesh_name(&stat_rec, 1, false);
                    m16 = get_lod_mesh_name(&stat_rec, 2, false);
                    m32 = get_lod_mesh_name(&stat_rec, 3, false);
                    if !m4.is_empty() || !m8.is_empty() || !m16.is_empty() || !m32.is_empty() {
                        s = format!(
                            "{}\t{}\t\t{}\t{m4}\t{m8}\t{m16}\t{m32}",
                            stat_rec.get_editor_id(),
                            hex8(flags(&stat_rec)),
                            get_lod_mesh_name(&stat_rec, -1, false)
                        );
                    }
                }
                let k = self.cache.len();
                self.cache.add_object(&s, key);
                k
            }
        };
        let mut s = self.cache.get(k).to_owned();
        if s.is_empty() {
            return;
        }
        // KYWD MultirefLOD: the levels of the linked reference's base
        // record are left to it
        let mut multi_ref_lod: Option<MainRecordRef> = None;
        if e.get_element_exists("Linked References")
            && let Some(entries) = e.get_element_by_name("Linked References")
        {
            for entry in children(&*entries) {
                let Some(entry) = entry.as_container() else {
                    continue;
                };
                let keyword = entry
                    .get_element_links_to("Keyword/Ref")
                    .and_then(|k| k.into_main_record());
                if keyword.is_some_and(|keyword| keyword.get_editor_id() == "MultirefLOD") {
                    multi_ref_lod = entry
                        .get_element_links_to("Ref")
                        .and_then(|r| r.into_main_record())
                        .map(|r| r.get_winning_override())
                        .and_then(|r| r.get_base_record())
                        .map(|r| r.get_winning_override());
                    break;
                }
            }
            if let Some(multi) = &multi_ref_lod {
                let mut sl = delimited_text(&s, '\t');
                if sl.len() == 8 {
                    for level in 0..4 {
                        if !get_lod_mesh_name(multi, level, false).is_empty() {
                            sl[4 + level as usize] = String::new();
                        }
                    }
                }
                s = delimited_text_getter(&sl, ',').replace(',', "\t");
            }
        }
        let mut scl = e.get_element_edit_value("XSCL");
        if scl.is_empty() {
            scl = "1.0".to_owned();
        }
        // material swap on a reference, or on the base record
        let mswp = if e.get_element_exists("XMSP") {
            e.get_element_links_to("XMSP").and_then(|m| m.into_main_record())
        } else if stat_rec.get_element_exists("Model")
            && let Some(model) = stat_rec.get_element_by_name("Model")
            && let Some(model) = model.as_container()
            && model.get_element_exists("MODS")
        {
            model.get_element_links_to("MODS").and_then(|m| m.into_main_record())
        } else {
            None
        };
        let (mut basemat, mut swapmat) = (String::new(), String::new());
        if let Some(mswp) = mswp
            && let Some(entries) = mswp
                .get_winning_override()
                .get_element_by_name("Material Substitutions")
        {
            let (mut base, mut swap) = (Vec::new(), Vec::new());
            for entry in children(&*entries) {
                let Some(entry) = entry.as_container() else {
                    continue;
                };
                let value = |path: &str| {
                    entry
                        .get_element_by_path(path)
                        .map_or_else(String::new, |v| v.get_edit_value())
                };
                let b = get_asset_name(&value("BNAM"), "", AssetType::Material).to_lowercase();
                let s = get_asset_name(&value("SNAM"), "", AssetType::Material).to_lowercase();
                if s.contains("lod\\") {
                    self.bgsm.add(&s);
                }
                swap.push(s);
                base.push(b);
            }
            basemat = delimited_text_getter(&base, ',');
            swapmat = delimited_text_getter(&swap, ',');
        }
        let pos = |axis: &str| e.get_element_edit_value(&format!("DATA\\Position\\{axis}"));
        let rot = |axis: &str| e.get_element_edit_value(&format!("DATA\\Rotation\\{axis}"));
        self.refs.add(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{scl}\t{s}\t{xespid}\t{basemat}\t{swapmat}\t{transrot}",
            e.get_load_order_form_id().to_string(false),
            hex8(flags(e)),
            pos("X"),
            pos("Y"),
            pos("Z"),
            rot("X"),
            rot("Y"),
            rot("Z")
        ));
        // list of used LOD meshes for atlas
        if !m4.is_empty() {
            self.lod_meshes.add(&m4);
        }
        if !m8.is_empty() {
            self.lod_meshes.add(&m8);
        }
        if !m16.is_empty() {
            self.lod_meshes.add(&m16);
        }
        // UPSTREAM-QUIRK: a level 32 model adds the level 16 model.
        if !m32.is_empty() {
            self.lod_meshes.add(&m16);
        }
        // process the next placements of this part of a SCOL
        if !recursive && i_placement < max_placement {
            for n in i_placement..max_placement {
                self.process_reference(e, i_part, n + 1, true);
            }
        }
    }
}

/// `wbGenerateLODFO4`: the objects LOD of a Fallout 4 worldspace.
pub fn generate_lod_fo4(env: &LodEnv, worldspace: &MainRecordRef) -> LodResult<()> {
    let Some(lod_set) = lod_settings(worldspace)? else {
        return Ok(());
    };
    let section = env.section();
    let refrs = find_unique_worldspace_refrs(worldspace);
    if refrs.is_empty() {
        return Ok(());
    }
    let id = worldspace.get_editor_id();
    message(&format!("[{id}] Generating LOD"));
    let chunk = env.read_bool(&section, "Chunk", false);
    let build_atlas = env.read_bool(&section, "BuildAtlas", true);
    let mut chunk_sw = (i32::MIN, i32::MIN);
    let mut chunk_size = 32;
    // Outside the handler of the generator, as upstream reads them.
    if chunk {
        let level = env.read_string(&section, "LODLevel", "");
        if !level.is_empty() {
            chunk_size = str_to_int(&level)?;
        }
        let x = env.read_string(&section, "LODX", &i32::MIN.to_string());
        if !x.is_empty() {
            chunk_sw.0 = str_to_int(&x)?;
        }
        let y = env.read_string(&section, "LODY", &i32::MIN.to_string());
        if !y.is_empty() {
            chunk_sw.1 = str_to_int(&y)?;
        }
    }
    let result = (|| -> LodResult<()> {
        let mut state = Fo4State {
            lod_set,
            chunk,
            build_atlas,
            chunk_sw,
            chunk_ne: (chunk_sw.0.wrapping_add(chunk_size), chunk_sw.1.wrapping_add(chunk_size)),
            cache: StringList::new(),
            refs: StringList::new(),
            lod_meshes: StringList::sorted(),
            bgsm: StringList::sorted(),
        };
        for refr in &refrs {
            state.process_reference(refr, 0, 0, false);
        }
        if state.refs.is_empty() {
            message(&format!(
                "<Note: Can not build Objects LOD for {id}, no valid references found>"
            ));
            return Ok(());
        }
        let mut uv_range = xedit_core::delphi::str_to_float(&env.read_string(&section, "AtlasTextureUVRange", "1.5"))
            .unwrap_or(1.5) as f32;
        let atlas_name;
        let atlas_map_name;
        if build_atlas {
            atlas_name = format!("{}textures\\terrain\\{id}\\Objects\\{id}Objects.dds", env.output_path);
            atlas_map_name = format!("{}LODGenAtlasMap.txt", env.scripts_path);
            let folder = extract_file_path(&atlas_name);
            if !std::path::Path::new(folder).is_dir() && !force_directories(folder) {
                return Err(LodError::new(format!(
                    "Can not create output folder for atlas {folder}"
                )));
            }
        } else {
            atlas_name = String::new();
            atlas_map_name = format!("{}{}-AtlasMap-{id}.txt", env.scripts_path, app_name());
            uv_range = 10000.0;
        }
        let mut export = StringList::new();
        export.add(&format!("GameMode={}", app_name()));
        export.add(&format!("Worldspace={id}"));
        export.add(&format!("CellSW={} {}", lod_set.sw_cell.0, lod_set.sw_cell.1));
        export.add("RemoveUnseenFaces=True");
        let no_lod_water = worldspace.get_element_native_value("DATA\\No LOD Water");
        export.add(&format!(
            "IgnoreWater={}",
            boolean_text(no_lod_water.as_ordinal().unwrap_or(0) != 0)
        ));
        export.add(&format!(
            "DontGenerateVertexColors={}",
            boolean_text(env.read_bool(&section, "ObjectsNoVertexColors", false))
        ));
        export.add(&format!(
            "DontGenerateTangents={}",
            boolean_text(env.read_bool(&section, "ObjectsNoTangents", false))
        ));
        export.add(&format!(
            "DefaultAlphaThreshold={}",
            env.read_integer(&section, "DefaultAlphaThreshold", env.defaults.alpha_threshold)
        ));
        export.add(&format!(
            "UseAlphaThreshold={}",
            boolean_text(env.read_bool(&section, "ObjectsUseAlphaThreshold", false))
        ));
        export.add(&format!(
            "UseBacklightPower={}",
            boolean_text(env.read_bool(&section, "ObjectsUseBacklightPower", false))
        ));
        if !atlas_map_name.is_empty() {
            export.add(&format!("TextureAtlasMap={atlas_map_name}"));
            export.add(&format!(
                "AtlasTolerance={}",
                float_to_str_f_fixed(f64::from(uv_range) - 1.0, 1)
            ));
        }
        export.add(&format!("PathData={}", env.data_path));
        export.add(&format!("PathOutput={}meshes\\terrain\\{id}\\Objects", env.output_path));
        resource_lines(&mut export);
        extra_options(env, worldspace, &mut export);
        for line in state.refs.strings() {
            export.add(line);
        }
        let export_name = format!("{}LODGen.txt", env.scripts_path);
        message(&format!("[{id}] Saving LODGen data: {export_name}"));
        export.save_to_file(&export_name)?;
        if build_atlas {
            let mut lod_textures = StringList::sorted();
            get_uv_range_textures_list(&state.lod_meshes, &mut lod_textures, uv_range)?;
            // textures from LOD material swaps
            if !state.bgsm.is_empty() {
                // The diffuse texture of the reused material file.
                let mut last_diffuse = String::new();
                let mut list = StringList::new();
                list.compare = ListCompare::Ascii;
                let containers = xedit_core::container_handler::container_list();
                for container in containers.iter().rev() {
                    for name in xedit_core::container_handler::container_resource_list(container, "materials\\lod\\") {
                        list.add(&name);
                    }
                }
                list.ignore_duplicates = true;
                list.sort();
                let mut i: i64 = -1;
                while i < state.bgsm.len() as i64 - 1 {
                    i += 1;
                    let current = state.bgsm.get(i as usize).to_owned();
                    // replace wildcard swaps with matching files
                    if current.contains('*') {
                        for name in list.strings().map(str::to_owned).collect::<Vec<_>>() {
                            if matches_mask(&name, &current) {
                                state.bgsm.add(&name);
                            }
                        }
                        continue;
                    }
                    let mut s = current.clone();
                    // fix bug in dlcnukaworld xx01F5B3
                    if s == "materials\\dlc04\\lod\\architecture\\galacticzone\\metalpaneltrim01_lod__black17.bgsm" {
                        s = "materials\\dlc04\\lod\\architecture\\galacticzone\\metalpaneltrim01_lod_black17.bgsm"
                            .to_owned();
                    }
                    // UPSTREAM-QUIRK: the material of the last material that
                    // loaded stays in the reused material file when one
                    // fails, and its diffuse texture is added again.
                    if !resource_exists(&s) {
                        message(&format!("<Warning: Error reading \"{s}\": Not found>"));
                    } else {
                        match load_material(&s) {
                            Ok(diffuse) => last_diffuse = diffuse,
                            Err(error) => message(&format!("<Warning: Error reading \"{s}\": {error}>")),
                        }
                    }
                    lod_textures.add(&get_asset_name(&last_diffuse, "", AssetType::Texture));
                }
            }
            if lod_textures.len() > 1 {
                message(&format!("[{id}] Building LOD textures atlas: {atlas_name}"));
                let size = env.read_integer(&section, "AtlasTextureSize", 512);
                build_atlas_from_textures_list(
                    env,
                    &lod_textures,
                    size,
                    size,
                    env.read_integer(&section, "AtlasWidth", 4096),
                    env.read_integer(&section, "AtlasHeight", 4096),
                    &atlas_name,
                    &atlas_map_name,
                )?;
            }
        }
        let mut command = format!("\"{}{LODGEN_NAME}\" \"{export_name}\"", env.scripts_path);
        if chunk {
            for (option, ident) in [("--lodLevel", "LODLevel"), ("--x", "LODX"), ("--y", "LODY")] {
                let value = env.read_string(&section, ident, "");
                if !value.is_empty() {
                    command.push_str(&format!(" {option} {value}"));
                }
            }
        }
        message(&format!("[{id}] Running {command}"));
        run_lodgen(&command)?;
        message(&format!("[{id}] Objects LOD Done."));
        Ok(())
    })();
    if let Err(error) = result {
        message(&format!("[{id}] Objects LOD generation error: {error}"));
    }
    Ok(())
}

/// `StrToInt`: the exception of a text that is not a number.
fn str_to_int(text: &str) -> LodResult<i32> {
    crate::variant::str_to_int(text).ok_or_else(|| LodError::new(format!("'{text}' is not a valid integer value")))
}
