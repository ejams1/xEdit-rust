// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Sniff/Proc/ProcHavokInfo.pas

//! `Havok information`: reports the mass, layer and shape of each rigid
//! body with the fields chosen, and the count of static and dynamic bodies
//! with their total mass.

use xedit_io::encoding::ansi_compare_text;

use crate::data_format::{R, df_float_to_str};
use crate::data_format_nif::{NifFile, block_is_dynamic_rigid_body, block_type, blocks_by_type};
use crate::proc_base;
use crate::sniff::processor::{
    GameType, Proc, ProcBase, ProcContext, ProcFileObject, Storage, access_violation, ansi_same_text, comma_text,
};

/// The fields of `lvFields`.
const FIELDS: [&str; 16] = [
    "Inertia Tensor",
    "Linear Damping",
    "Angular Damping",
    "Time Factor",
    "Gravity Factor",
    "Friction",
    "Rolling Friction Multiplier",
    "Restitution",
    "Max Linear Velocity",
    "Max Angular Velocity",
    "Penetration Depth",
    "Motion System",
    "Deactivator Type",
    "Enable Deactivation",
    "Solver Deactivation",
    "Motion Quality",
];

pub struct ProcHavokInfo {
    base: ProcBase,
    per_object_checked: bool,
    same_line_checked: bool,
    /// The checked items of `lvFields`.
    fields_checked: Vec<&'static str>,
    per_object: bool,
    same_line: bool,
    fields: Vec<&'static str>,
}

impl ProcHavokInfo {
    pub fn new() -> ProcHavokInfo {
        let mut base = ProcBase::new(
            "Havok information",
            &[
                GameType::Tes4,
                GameType::Fo3,
                GameType::Fnv,
                GameType::Tes5,
                GameType::Sse,
            ],
            &["nif"],
        );
        base.no_output = true;
        ProcHavokInfo {
            base,
            per_object_checked: true,
            same_line_checked: false,
            fields_checked: Vec::new(),
            per_object: true,
            same_line: false,
            fields: Vec::new(),
        }
    }
}

impl Proc for ProcHavokInfo {
    proc_base!();

    fn on_show(&mut self, storage: &Storage) {
        self.per_object_checked = storage.get_bool("bPerObject", true);
        self.same_line_checked = storage.get_bool("bSameLine", false);
        let checked = comma_text(&storage.get_string("sFields", ""));
        self.fields_checked = FIELDS
            .iter()
            .filter(|field| checked.iter().any(|name| ansi_same_text(name, field)))
            .copied()
            .collect();
    }

    fn on_start(&mut self) -> R<()> {
        self.per_object = self.per_object_checked;
        self.same_line = self.same_line_checked;
        self.fields = self.fields_checked.clone();
        Ok(())
    }

    fn process_file(&self, file: &mut ProcFileObject, ctx: &mut ProcContext) -> R<Vec<u8>> {
        let mut log: Vec<String> = Vec::new();
        let mut nif = NifFile::new()?;
        nif.load_from_data(&file.get_data()?)?;
        let tree = &mut nif.tree;
        let (mut statics, mut dynamics) = (0, 0);
        let mut mass: f32 = 0.0;
        for col in blocks_by_type(tree, "bhkCollisionObject", true)? {
            let body = tree.elements(col, "Body")?.ok_or_else(access_violation)?;
            let Some(rigid) = tree.links_to(body)? else { continue };
            let target_link = tree.elements(col, "Target")?.ok_or_else(access_violation)?;
            let name = match tree.links_to(target_link)? {
                Some(target) => {
                    let name = tree.edit_values(target, "Name")?;
                    if name.is_empty() { tree.name(target)? } else { name }
                }
                None => "<No target>".to_owned(),
            };
            let shape_link = tree.elements(rigid, "Shape")?.ok_or_else(access_violation)?;
            let mut shape = tree.links_to(shape_link)?;
            if let Some(transform) = shape
                && block_type(tree, transform) == "bhkTransformShape"
            {
                let link = tree.elements(transform, "Shape")?.ok_or_else(access_violation)?;
                shape = tree.links_to(link)?;
            }
            let shape_type = match shape {
                Some(shape) => block_type(tree, shape).to_owned(),
                None => "<No shape>".to_owned(),
            };

            if self.per_object {
                let mut line = format!(
                    "\t{name}      {}    {}    {shape_type}",
                    tree.edit_values(rigid, "Mass")?,
                    tree.edit_values(rigid, "Havok Filter\\Layer")?
                );
                for field in &self.fields {
                    let Some(el) = tree.elements(rigid, field)? else {
                        continue;
                    };
                    let value = if *field == "Inertia Tensor" {
                        format!(
                            "\"{} {} {}\"",
                            tree.edit_values(el, "m11")?,
                            tree.edit_values(el, "m22")?,
                            tree.edit_values(el, "m33")?
                        )
                    } else {
                        tree.edit_value(el)?
                    };
                    if self.same_line {
                        line.push_str(&format!("    {value}"));
                    } else {
                        line.push_str(&format!("\r\n\t\t{field}:\t{value}"));
                    }
                }
                log.push(line);
            }

            if block_is_dynamic_rigid_body(tree, rigid)? {
                dynamics += 1;
            } else {
                statics += 1;
            }
            mass = (f64::from(mass) + tree.native_values(rigid, "Mass")?.to_f64()?) as f32;
        }

        // `TStringList.Sort`.
        log.sort_by(|a, b| ansi_compare_text(a, b));
        if statics + dynamics > 0 {
            log.push(format!(
                "\tStatic: {statics}    Dynamic: {dynamics}    Total Mass: {}",
                df_float_to_str(f64::from(mass))
            ));
        }
        if !log.is_empty() {
            log.insert(0, file.file_name.clone());
            log.push(String::new());
            ctx.add_messages(log);
        }
        Ok(Vec::new())
    }
}
