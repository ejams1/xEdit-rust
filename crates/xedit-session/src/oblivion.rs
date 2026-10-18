// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavBanditFixClick,
// mniNavRaceLVLIsClick)

//! The two Oblivion helpers of the main form: the bandit script fix of
//! Mart's Monster Mod and the race leveled item lists of the NPCs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_core::implementation::MainRecordImpl;
use xedit_core::interface::{Container, Element, MainRecord, MainRecordRef, Signature};

use crate::formids::{RecordRef, record_ref};
use crate::{CommandError, Registry, Session};

/// `StrContaines`: whether `text` contains `needle` without regard to case.
fn contains_text(text: &str, needle: &str) -> bool {
    text.to_lowercase().contains(&needle.to_lowercase())
}

fn impl_of(record: &MainRecordRef) -> Result<std::sync::Arc<MainRecordImpl>, CommandError> {
    crate::formids::record_impl(record)
}

/// `records.bandit_fix`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BanditFixRequest {
    /// Report the records, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// A record the bandit fix changed.
#[derive(Serialize, JsonSchema)]
pub struct BanditFixChange {
    #[serde(flatten)]
    pub record: RecordRef,
    /// The script FormID before and after, as eight hexadecimal digits.
    pub old_script: String,
    pub new_script: String,
}

/// `records.bandit_fix`: the response.
#[derive(Serialize, JsonSchema)]
pub struct BanditFixResponse {
    /// The NPC_ records whose `SCRI` changed (or would).
    pub changed: Vec<BanditFixChange>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

/// `records.bandit_fix`: the bandit fix of Mart's Monster Mod
/// (`mniNavBanditFixClick`). The scripts `CSNPC`, `CSNPCBoss`,
/// `CSNPCBandit` and `CSNPCBanditBoss` of `Mart's Monster Mod.esm` are
/// looked up, then every `NPC_` of that module and of the modules that have
/// it as a master whose editor ID names a bandit, marauder or raider (but
/// not an adventurer) gets `CSNPC` replaced by `CSNPCBandit`, `CSNPCBoss` by
/// `CSNPCBanditBoss`, and an empty `SCRI` by the bandit or bandit boss
/// script of its `Boss` editor ID.
fn records_bandit_fix(session: &mut Session, request: BanditFixRequest) -> Result<BanditFixResponse, CommandError> {
    let mmm_index = session
        .files
        .iter()
        .position(|file| file.get_name().eq_ignore_ascii_case("Mart's Monster Mod.esm"))
        .ok_or_else(|| CommandError::new("missing_file", "Can't find Mart's Monster Mod.esm"))?;
    let mmm = session.files[mmm_index].clone();
    // UPSTREAM-QUIRK: the release checks `Assigned(CSNPC)` for all four
    // scripts, so a missing boss or bandit script is not reported and
    // crashes at the first use. The port reports the script the release
    // meant to name.
    // `Files[MMMESM].GroupBySignature['SCPT']` with the editor ID compared:
    // the release does not use the editor ID index here.
    let script_of = |editor_id: &str| {
        mmm.records()
            .into_iter()
            .find(|record| {
                record.get_signature().to_string() == "SCPT" && record.get_editor_id().eq_ignore_ascii_case(editor_id)
            })
            .map(|record| record as MainRecordRef)
            .ok_or_else(|| {
                CommandError::new(
                    "missing_record",
                    format!("Can't find {editor_id} script in Mart's Monster Mod.esm"),
                )
            })
    };
    let csnpc = script_of("CSNPC")?;
    let csnpc_boss = script_of("CSNPCBoss")?;
    let csnpc_bandit = script_of("CSNPCBandit")?;
    let csnpc_bandit_boss = script_of("CSNPCBanditBoss")?;
    let id = |record: &MainRecordRef| record.get_load_order_form_id().to_string(false);
    let csnpc_id = id(&csnpc);
    let csnpc_boss_id = id(&csnpc_boss);
    let csnpc_bandit_id = id(&csnpc_bandit);
    let csnpc_bandit_boss_id = id(&csnpc_bandit_boss);
    let mut changed = Vec::new();
    for (index, file) in session.files.iter().enumerate() {
        if index != mmm_index && !file.has_master("Mart's Monster Mod.esm") {
            continue;
        }
        for record in file.records() {
            if record.get_signature().to_string() != "NPC_" {
                continue;
            }
            let record: MainRecordRef = record as MainRecordRef;
            let editor_id = record.get_editor_id();
            if contains_text(&editor_id, "Adven")
                || !(contains_text(&editor_id, "Bandit")
                    || contains_text(&editor_id, "Maraud")
                    || contains_text(&editor_id, "Raider"))
            {
                continue;
            }
            let record_impl = impl_of(&record)?;
            // The native value is the FormID; the edit value of a form link
            // formats the target's name here.
            let old_script = record_impl
                .get_element_native_value("SCRI")
                .as_ordinal()
                .filter(|value| *value != 0)
                .map_or_else(String::new, |value| format!("{:08X}", value as u32));
            let new_script = if old_script.is_empty() {
                if contains_text(&editor_id, "Boss") {
                    csnpc_bandit_boss_id.clone()
                } else {
                    csnpc_bandit_id.clone()
                }
            } else if old_script.trim() == csnpc_id {
                csnpc_bandit_id.clone()
            } else if old_script.trim() == csnpc_boss_id {
                csnpc_bandit_boss_id.clone()
            } else {
                continue;
            };
            if !request.dry_run {
                record_impl
                    .set_element_edit_value("SCRI", &new_script)
                    .map_err(|error| CommandError::new("edit_failed", error))?;
            }
            changed.push(BanditFixChange {
                record: record_ref(&record_impl),
                old_script,
                new_script,
            });
        }
    }
    Ok(BanditFixResponse {
        changed,
        dry_run: request.dry_run,
    })
}

/// `records.race_leveled_lists`: the request.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RaceLeveledListsRequest {
    /// Report the records, change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// A record the race leveled list helper changed.
#[derive(Serialize, JsonSchema)]
pub struct RaceLeveledListChange {
    pub record: RecordRef,
    pub race: String,
    /// The leveled item's editor ID and the FormID the item now names.
    pub leveled_list: String,
    pub old_form_id: String,
    pub new_form_id: String,
}

/// `records.race_leveled_lists`: the response.
#[derive(Serialize, JsonSchema)]
pub struct RaceLeveledListsResponse {
    pub changed: Vec<RaceLeveledListChange>,
    pub dry_run: bool,
}

/// `records.race_leveled_lists`: the race leveled item lists of the NPCs
/// (`mniNavRaceLVLIsClick`). Every `LVLI` a loaded file defines (a master
/// with an editor ID) is indexed by its editor ID and load order; every
/// `NPC_` of a loaded file whose items entry names such a list under the
/// editor ID of its race gets the list's load order FormID as the entry's
/// FormID (`Items[i].Elements[0].EditValue`), when the list's load order is
/// not after the NPC's file.
fn records_race_leveled_lists(
    session: &mut Session,
    request: RaceLeveledListsRequest,
) -> Result<RaceLeveledListsResponse, CommandError> {
    // `LVLIs`: the editor ID of every master LVLI of the loaded files with a
    // non-empty one, the first of a name winning (`Sorted, dupIgnore`,
    // compared without case), with its load order FormID and load order.
    let mut lists: Vec<(String, String, i32)> = Vec::new();
    for file in &session.files {
        for record in file.records() {
            if record.get_signature().to_string() != "LVLI" {
                continue;
            }
            let record_handle: MainRecordRef = record as MainRecordRef;
            let editor_id = record_handle.get_editor_id();
            if !record_handle.get_is_master() || editor_id.is_empty() {
                continue;
            }
            // `dupIgnore`: a name added before keeps its first entry.
            if lists.iter().any(|(name, _, _)| name.eq_ignore_ascii_case(&editor_id)) {
                continue;
            }
            lists.push((
                editor_id,
                record_handle.get_load_order_form_id().to_string(false),
                file.load_order(),
            ));
        }
    }
    let mut changed = Vec::new();
    for file in session.files.clone() {
        let file_load_order = file.load_order();
        for record in file.records() {
            if record.get_signature().to_string() != "NPC_" {
                continue;
            }
            let record_handle: MainRecordRef = record as MainRecordRef;
            let Some(rnam) = record_handle
                .as_container()
                .and_then(|record| record.get_element_by_signature(Signature::new(b"RNAM")))
            else {
                continue;
            };
            let race = rnam
                .get_links_to()
                .and_then(|target| target.into_main_record())
                .map(|race| race.get_editor_id())
                .unwrap_or_default();
            if race.is_empty() {
                continue;
            }
            let Some(items) = record_handle
                .as_container()
                .and_then(|record| record.get_element_by_name("Items"))
            else {
                continue;
            };
            let Some(items) = items.as_container() else {
                continue;
            };
            for index in 0..items.get_element_count() {
                let Some(item) = items.get_element(index) else {
                    continue;
                };
                let Some(item) = item.as_container() else {
                    continue;
                };
                if item.get_element_count() != 2 {
                    continue;
                }
                let Some(reference) = item.get_element(0) else {
                    continue;
                };
                let Some(target) = reference.get_links_to().and_then(|target| target.into_main_record()) else {
                    continue;
                };
                if target.get_signature().to_string() != "LVLI" {
                    continue;
                }
                let leveled_list = target.get_editor_id();
                // UPSTREAM-QUIRK: the release looks the list up under
                // `EditorID+Race` (`LVLIs.Find(MainRecord2.EditorID+Race)`),
                // but the list holds the editor IDs alone
                // (`LVLIs.AddObject(MainRecord.EditorID, ...)`), so no
                // entry matches and the item changes nothing outside a list
                // whose editor ID happens to be such a concatenation. The
                // port follows it.
                let key = format!("{leveled_list}{race}");
                let Some((_, form_id, load_order)) = lists.iter().find(|(name, _, _)| name.eq_ignore_ascii_case(&key))
                else {
                    continue;
                };
                if *load_order > file_load_order {
                    continue;
                }
                let old_form_id = reference.get_edit_value();
                if !request.dry_run {
                    reference
                        .set_edit_value(form_id)
                        .map_err(|error| CommandError::new("edit_failed", error))?;
                }
                if old_form_id == *form_id {
                    continue;
                }
                let record_impl = impl_of(&record_handle)?;
                changed.push(RaceLeveledListChange {
                    record: record_ref(&record_impl),
                    race: race.clone(),
                    leveled_list,
                    old_form_id,
                    new_form_id: form_id.clone(),
                });
            }
        }
    }
    Ok(RaceLeveledListsResponse {
        changed,
        dry_run: request.dry_run,
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "records.bandit_fix",
        "The bandit script fix of Mart's Monster Mod (mniNavBanditFix).",
        true,
        records_bandit_fix,
    );
    registry.register(
        "records.race_leveled_lists",
        "Point the NPCs' race leveled item entries at the master leveled lists by their load order FormID (mniNavRaceLVLIs).",
        true,
        records_race_leveled_lists,
    );
}
