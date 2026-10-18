// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbHelpers.pas (wbLeveledListCheckCircular) and
// xEdit/xeMainForm.pas (mniNavCheckForCircularLeveledListsClick)

//! `leveled_lists.check_circular`: the "Check for Circular Leveled Lists"
//! item. Every leveled list of the loaded files (`LVLI`, `LVLC`, `LVLN` and
//! `LVSP`) is walked through the records its entries refer to, and a list
//! that refers back to a list already on the walk is reported with the path
//! from the first occurrence to the repeat. The release tags the records it
//! walks (`ResetAllTags` and `Tag`); the port keeps the same set of visited
//! records locally.

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::MainRecordImpl;
use xedit_core::interface::{Element, MainRecord};

use crate::{CommandError, Registry, Session};

/// The reference path of the entries of a leveled list, from
/// `wbLeveledListCheckCircular`: the base name of the record's signature.
fn reference_path(signature: &str) -> Option<&'static str> {
    Some(match signature {
        "LVLB" => "Base Form",
        "LVLC" => "Creature",
        "LVLI" => "Item",
        "LVLN" => "NPC",
        "LVLP" => "Pack In",
        "LVPC" => "Perk Card",
        "LVSC" => "Space Cell",
        "LVSP" => "Spell",
        _ => return None,
    })
}

/// Port of `wbLeveledListCheckCircular`: `stack` is the walk so far and
/// `visited` the tagged records of the whole check.
fn check_circular(
    record: &std::sync::Arc<MainRecordImpl>,
    stack: &mut Vec<std::sync::Arc<MainRecordImpl>>,
    visited: &mut HashSet<u32>,
) -> Result<(), String> {
    let form_id = record.get_load_order_form_id().to_cardinal();
    if let Some(index) = stack
        .iter()
        .position(|parent| parent.get_load_order_form_id().to_cardinal() == form_id)
    {
        let mut message = record.get_name();
        for parent in stack[index..].iter().rev() {
            message = format!("{} -> {message}", parent.get_name());
        }
        return Err(format!("Circular Leveled List found: {message}"));
    }
    if !visited.insert(form_id) {
        return Ok(());
    }
    let signature = record.get_signature().to_string();
    let Some(base) = reference_path(&signature) else {
        return Ok(());
    };
    let ref_path = if xedit_core::interface::globals::is_oblivion() {
        base.to_owned()
    } else {
        format!("LVLO\\{base}")
    };
    let record_ref: xedit_core::interface::ElementRef = record.clone();
    let Some(entries) = record_ref
        .as_container()
        .and_then(|record| record.get_element_by_name("Leveled List Entries"))
    else {
        return Ok(());
    };
    let Some(entries) = entries.as_container() else {
        return Ok(());
    };
    stack.push(record.clone());
    let mut result = Ok(());
    for index in 0..entries.get_element_count() {
        let Some(entry) = entries.get_element(index) else {
            continue;
        };
        let Some(reference) = entry
            .as_container()
            .and_then(|entry| entry.get_element_by_path(&ref_path))
        else {
            continue;
        };
        let Some(target) = reference.get_links_to().and_then(|target| target.into_main_record()) else {
            continue;
        };
        if target.get_signature().to_string() != signature {
            continue;
        }
        let winning_ref = target.get_winning_override();
        let Ok(winning) = crate::formids::record_impl(&winning_ref) else {
            continue;
        };
        result = check_circular(&winning, stack, visited);
        if result.is_err() {
            break;
        }
    }
    stack.pop();
    result
}

/// `leveled_lists.check_circular`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckCircularRequest {}

/// `leveled_lists.check_circular`: the response.
#[derive(Serialize, JsonSchema)]
pub struct CheckCircularResponse {
    /// `Circular Leveled List found: ...` for every cycle, in the order the
    /// walk finds them.
    pub cycles: Vec<String>,
    /// How many leveled lists of the loaded files the walk started from.
    pub checked: usize,
}

fn check_circular_command(
    session: &mut Session,
    _request: CheckCircularRequest,
) -> Result<CheckCircularResponse, CommandError> {
    session.mode()?;
    let mut visited = HashSet::new();
    let mut cycles = Vec::new();
    let mut checked = 0;
    for file in &session.files {
        for record in file.records() {
            if !matches!(
                record.get_signature().to_string().as_str(),
                "LVLI" | "LVLC" | "LVLN" | "LVSP"
            ) {
                continue;
            }
            checked += 1;
            let winning_ref = record.get_winning_override();
            let Ok(winning) = crate::formids::record_impl(&winning_ref) else {
                continue;
            };
            let mut stack = Vec::new();
            if let Err(error) = check_circular(&winning, &mut stack, &mut visited) {
                cycles.push(error);
            }
        }
    }
    Ok(CheckCircularResponse { cycles, checked })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "leveled_lists.check_circular",
        "Walk the leveled lists of the loaded files and report the ones that refer back to themselves (Check for Circular Leveled Lists).",
        false,
        check_circular_command,
    );
}
