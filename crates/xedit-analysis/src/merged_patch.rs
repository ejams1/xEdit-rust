// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniNavCreateMergedPatchClick
// with UpdateOrderedTargetList, UpdateTargetList, ListsEqual, CheckGroup and
// BuildList)

//! "Create Merged Patch": the lists that several plugins change in the same
//! record (leveled list entries, container items, faction relations, the
//! hairs and eyes of a race, form lists, the items, factions, spells, perks
//! and keywords of an actor, the keywords of items and effects) merged into
//! one override in a new plugin, so that no plugin's additions or removals
//! are lost to the plugin that loads last.
//!
//! For each record with at least two overrides the handler builds, for
//! every list it merges, the list of the master and replays on it what each
//! override changed against the version in its own masters
//! (`UpdateTargetList`: entries removed, added, and changed entries taken).
//! When the result is not the list of the winning override, the winning
//! override is copied into the patch and its list is replaced by the merged
//! one, with the counter of the list set to the new entry count. Entries
//! are compared by their extended sort keys (`DisplaySortKey[True]`), and a
//! list is merged only when its array is sorted, or when it is the list of
//! a form list whose editor ID ends in `OrderedList` (where the overrides
//! may only append, `UpdateOrderedTargetList`).
//!
//! The selection is upstream's: the record has two overrides or more; the
//! conflict status of the records and the mod groups play no part (the
//! 4.1.5q handler reads `Overrides` of the master, which mod groups do not
//! change, so a patch made with mod groups active is the patch made
//! without).
//!
//! The string lists of the handler are `TStringList`s: their sort and their
//! `Find` compare with the locale (`AnsiCompareText`,
//! [`xedit_io::collate::ansi_compare_text`]), the walk of `UpdateTargetList`
//! with `CompareText`, and `Find` searches by halves whether or not the
//! list is sorted. [`StringList`] keeps those quirks, because they decide
//! which entries the patch gets.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;

use xedit_core::implementation::copy::{copy_element_to_file, copy_element_to_record};
use xedit_core::implementation::sortable::{sortable, view_elements};
use xedit_core::implementation::{FileImpl, MainRecordImpl};
use xedit_core::interface::globals::{GameMode, allow_internal_edit, game_mode, set_allow_internal_edit};
use xedit_core::interface::misc::Variant;
use xedit_core::interface::types::{ElementType, Signature};
use xedit_core::interface::{Container, CopyArgs, Element, ElementRef, MainRecord};
use xedit_io::collate::ansi_compare_text;

/// The suffix of the editor ID of a form list whose order matters.
const ORDERED_LIST: &str = "OrderedList";

/// One `CheckGroup` call of the handler: the group, the lists merged in its
/// records and the counters of those lists (`''` for none), and whether the
/// lists are sets (`aAsSet`).
struct GroupMerge {
    signature: &'static [u8; 4],
    lists: &'static [&'static str],
    counts: &'static [&'static str],
    as_set: bool,
}

const fn merge(
    signature: &'static [u8; 4],
    lists: &'static [&'static str],
    counts: &'static [&'static str],
) -> GroupMerge {
    GroupMerge {
        signature,
        lists,
        counts,
        as_set: false,
    }
}

/// The `CheckGroup` calls of `mniNavCreateMergedPatchClick` for the game,
/// in their order.
fn group_merges(mode: GameMode) -> Vec<GroupMerge> {
    let mut merges = vec![
        merge(b"LVLI", &["Leveled List Entries"], &["LLCT"]),
        merge(b"LVLC", &["Leveled List Entries"], &["LLCT"]),
        merge(b"LVLN", &["Leveled List Entries"], &["LLCT"]),
        merge(b"LVSP", &["Leveled List Entries"], &["LLCT"]),
        merge(b"CONT", &["Items"], &["COCT"]),
        merge(b"FACT", &["Relations"], &[]),
        merge(
            b"RACE",
            &["HNAM - Hairs", "ENAM - Eyes", "Actor Effects"],
            &["", "", "SPCT"],
        ),
        GroupMerge {
            signature: b"FLST",
            lists: &["FormIDs"],
            counts: &[],
            as_set: true,
        },
        merge(b"CREA", &["Items", "Factions"], &["COCT"]),
    ];
    // "FNV doesn't merge DIAL quests properly at runtime"
    if mode == GameMode::gmFNV {
        merges.push(merge(b"DIAL", &["Added Quests"], &[]));
    }
    // "exclude Head Parts for Skyrim, causes issues"
    if mode >= GameMode::gmTES5 {
        merges.push(merge(
            b"NPC_",
            &["Items", "Factions", "Actor Effects", "Perks", "KWDA - Keywords"],
            &["COCT", "", "SPCT", "PRKZ", "KSIZ"],
        ));
    } else {
        merges.push(merge(
            b"NPC_",
            &["Items", "Factions", "Head Parts", "Actor Effects"],
            &[],
        ));
    }
    if mode >= GameMode::gmTES5 {
        for signature in [
            b"ALCH", b"ARMO", b"AMMO", b"BOOK", b"FLOR", b"FURN", b"INGR", b"MGEF", b"MISC", b"SCRL", b"SLGM", b"SPEL",
            b"WEAP",
        ] {
            merges.push(merge(signature, &["KWDA - Keywords"], &["KSIZ"]));
        }
    }
    merges
}

/// The message of the handler for the games it warns about
/// (`wbIsSkyrim or wbIsFallout4 or wbIsFallout76 or wbIsStarfield`); the
/// GUI asks whether to go on.
pub fn unsupported_warning(mode: GameMode, game_name2: &str) -> Option<String> {
    let warned = matches!(
        mode,
        GameMode::gmTES5
            | GameMode::gmEnderal
            | GameMode::gmTES5VR
            | GameMode::gmSSE
            | GameMode::gmEnderalSE
            | GameMode::gmFO4
            | GameMode::gmFO4VR
            | GameMode::gmFO76
            | GameMode::gmSF1
    );
    warned.then(|| {
        format!(
            "Merged patch is unsupported for {game_name2}. Create it only if you know what you are doing and can troubleshoot possible issues yourself. Do you want to continue?"
        )
    })
}

/// A list of a record that the patch merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedList {
    /// The name of the list, as the handler names it (`Leveled List Entries`).
    pub name: String,
    /// The entries of the merged list.
    pub entries: usize,
    /// The entries of the list of the winning override.
    pub winning_entries: usize,
}

/// A record the patch overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedRecord {
    /// The record as xEdit names it (`[LVLI:00012345] LLI_Something`), the
    /// version of the master.
    pub name: String,
    pub signature: Signature,
    /// The load order FormID.
    pub form_id: u32,
    /// The file of the winning override, which the patch copies.
    pub winning_file: String,
    pub lists: Vec<MergedList>,
}

/// What the merge did or, for a dry run, would do.
#[derive(Debug, Default)]
pub struct MergedPatchReport {
    pub records: Vec<MergedRecord>,
    /// The records with two overrides or more that were looked at.
    pub checked: usize,
    /// The lines the handler writes to the message log (`PostAddMessage`):
    /// faulty ordered lists and the messages of the exceptions it caught.
    pub messages: Vec<String>,
}

/// Port of the merge loop of `mniNavCreateMergedPatchClick`: `files` are the
/// loaded files in the order of the main form's `Files` (load order, the
/// hardcoded file after the game master), without the patch; the first is
/// skipped, as the handler skips it. With `target`, the merged records are
/// copied into it; without, nothing is changed and the report says what
/// would be merged.
pub fn merge_into(files: &[Arc<FileImpl>], target: Option<&Arc<FileImpl>>) -> MergedPatchReport {
    let mut report = MergedPatchReport::default();
    // `ResetAllTags`: no master is tagged yet.
    let mut tagged: HashSet<usize> = HashSet::new();
    let merges = group_merges(game_mode());
    let edit_state = allow_internal_edit();
    // "do not dynamically update counter fields, they are set by merging
    // code"
    set_allow_internal_edit(false);
    for file in files.iter().skip(1) {
        for merge in &merges {
            let Some(group) = file.group_by_signature(Signature::new(merge.signature)) else {
                continue;
            };
            let group: ElementRef = group;
            check_group(&group, merge, target, &mut tagged, &mut report);
        }
    }
    set_allow_internal_edit(edit_state);
    report
}

/// The `IwbContainerElementRef` test of `BuildList`: every element but a
/// flag and the terminator of a string list is a container.
fn is_container(element: &ElementRef) -> bool {
    element.as_container().is_some()
        && !matches!(
            element.get_element_type(),
            ElementType::etFlag | ElementType::etStringListTerminator
        )
}

/// Port of `BuildList`: the entries of a list by their extended sort keys.
/// A set without order is a sorted list without duplicates; any other list
/// is sorted and each key gets `#` and the count of the equal keys before
/// it, so that equal entries stay apart. A missing list gives an empty list
/// that is not sorted.
fn build_list(entries: Option<&ElementRef>, as_set: bool, ordered: bool) -> StringList {
    let mut result = StringList::new();
    if as_set && !ordered {
        result.set_sorted(true);
    }
    let Some(entries) = entries.filter(|entries| is_container(entries)) else {
        return result;
    };
    for entry in view_elements(entries) {
        if is_container(&entry) {
            let key = entry.get_sort_key(true);
            result.add_object(key, entry);
        }
    }
    if !as_set {
        result.sort();
        let mut last = String::new();
        let mut count = 0u64;
        for item in &mut result.items {
            // Delphi's `=` on strings: the code units.
            if item.0 == last {
                count += 1;
            } else {
                count = 0;
                last.clone_from(&item.0);
            }
            item.0 = format!("{}#{count:04X}", item.0);
        }
        result.set_sorted(true);
    }
    result
}

/// Port of `UpdateOrderedTargetList`: the entries the override appended.
fn update_ordered_target_list(left: &StringList, right: &StringList, target: &mut StringList) {
    for item in right.items.iter().skip(left.items.len()) {
        target.add_object(item.0.clone(), item.1.clone());
    }
}

/// Port of `UpdateTargetList`: what the override (`right`) changed against
/// its master (`left`), applied to `target`. The walk compares with
/// `CompareText`, the lists are sorted with the locale.
fn update_target_list(left: &StringList, right: &StringList, target: &mut StringList) {
    let (mut l, mut r) = (0, 0);
    while l < left.items.len() && r < right.items.len() {
        match compare_text(&left.items[l].0, &right.items[r].0) {
            Ordering::Less => {
                if let (true, index) = target.find(&left.items[l].0) {
                    target.delete(index);
                }
                l += 1;
            }
            Ordering::Equal => {
                if let (true, index) = target.find(&left.items[l].0) {
                    target.items[index].1 = right.items[r].1.clone();
                }
                l += 1;
                r += 1;
            }
            Ordering::Greater => {
                if !target.find(&right.items[r].0).0 {
                    target.add_object(right.items[r].0.clone(), right.items[r].1.clone());
                }
                r += 1;
            }
        }
    }
    while l < left.items.len() {
        if let (true, index) = target.find(&left.items[l].0) {
            target.delete(index);
        }
        l += 1;
    }
    while r < right.items.len() {
        if !target.find(&right.items[r].0).0 {
            target.add_object(right.items[r].0.clone(), right.items[r].1.clone());
        }
        r += 1;
    }
}

/// Port of `ListsEqual`: the same keys (`SameText`) in the same order; for
/// an ordered list, `left` may be shorter.
fn lists_equal(left: &StringList, right: &StringList, for_ordered_list: bool) -> bool {
    let fits = if for_ordered_list {
        left.items.len() <= right.items.len()
    } else {
        left.items.len() == right.items.len()
    };
    fits && left
        .items
        .iter()
        .zip(&right.items)
        .all(|(a, b)| compare_text(&a.0, &b.0) == Ordering::Equal)
}

/// Delphi's `CompareText` (and `SameText`): the ASCII letters compared as
/// upper case, everything else by code unit.
fn compare_text(a: &str, b: &str) -> Ordering {
    let fold = |unit: u16| {
        if (u16::from(b'a')..=u16::from(b'z')).contains(&unit) {
            unit - 32
        } else {
            unit
        }
    };
    a.encode_utf16().map(fold).cmp(b.encode_utf16().map(fold))
}

/// The lists of one record, while the handler builds them.
struct RecordLists {
    target: Option<StringList>,
    winning: Option<StringList>,
}

/// Port of `CheckGroup` for the main records of one group.
fn check_group(
    group: &ElementRef,
    merge: &GroupMerge,
    target: Option<&Arc<FileImpl>>,
    tagged: &mut HashSet<usize>,
    report: &mut MergedPatchReport,
) {
    let Some(container) = group.as_container() else {
        return;
    };
    for index in 0..container.get_element_count() {
        let Some(record) = container
            .get_element(index)
            .and_then(|element| element.as_element_impl()?.main_record_impl())
        else {
            continue;
        };
        let master = record.master_or_self_impl();
        if !tagged.insert(master.get_element_id()) {
            continue;
        }
        let overrides = master.overrides();
        if overrides.len() < 2 {
            continue;
        }
        report.checked += 1;
        if let Err(message) = check_record(&master, &overrides, merge, target, report) {
            report.messages.push(message);
        }
    }
}

/// The body of the loop of `CheckGroup` for one master record, whose
/// exceptions the handler writes to the log.
fn check_record(
    master: &Arc<MainRecordImpl>,
    overrides: &[Arc<MainRecordImpl>],
    merge: &GroupMerge,
    target: Option<&Arc<FileImpl>>,
    report: &mut MergedPatchReport,
) -> Result<(), String> {
    let mut faulty_ordered_list = false;
    let mut lists: Vec<RecordLists> = Vec::new();
    // The handler's `MainRecord` after the lists: the winning override
    // (`WinningOverride`, the last override).
    let winning = overrides.last().cloned().unwrap_or_else(|| master.clone());
    for name in merge.lists {
        let mut list = RecordLists {
            target: None,
            winning: None,
        };
        let master_list = master.get_element_by_name(name);
        let mut ordered = false;
        if let Some(sortable) = master_list.as_ref().and_then(sortable)
            && !sortable.sorted
        {
            let editor_id = master.get_editor_id();
            let tail: String = {
                let units: Vec<u16> = editor_id.encode_utf16().collect();
                let start = units.len().saturating_sub(ORDERED_LIST.len());
                if units.len() > ORDERED_LIST.len() {
                    String::from_utf16_lossy(&units[start..])
                } else {
                    editor_id.clone()
                }
            };
            ordered = compare_text(&tail, ORDERED_LIST) == Ordering::Equal;
            if !ordered {
                lists.push(list);
                continue;
            }
        }
        let mut target_list = build_list(master_list.as_ref(), merge.as_set, ordered);
        for record in overrides {
            let current = build_list(record.get_element_by_name(name).as_ref(), merge.as_set, ordered);
            let current_masters = xedit_core::implementation::master_records_from_master_files_and_self(record);
            let Some(current_master) = current_masters
                .iter()
                .rev()
                .find(|candidate| !Arc::ptr_eq(candidate, record))
            else {
                continue;
            };
            let current_master_list =
                build_list(current_master.get_element_by_name(name).as_ref(), merge.as_set, ordered);
            if ordered {
                if lists_equal(&current_master_list, &current, true) {
                    update_ordered_target_list(&current_master_list, &current, &mut target_list);
                } else {
                    faulty_ordered_list = true;
                }
            } else {
                update_target_list(&current_master_list, &current, &mut target_list);
            }
            if faulty_ordered_list {
                break;
            }
        }
        if faulty_ordered_list {
            break;
        }
        list.target = Some(target_list);
        list.winning = Some(build_list(
            winning.get_element_by_name(name).as_ref(),
            merge.as_set,
            ordered,
        ));
        lists.push(list);
    }
    if faulty_ordered_list {
        report
            .messages
            .push(format!("Error: Can't merge faulty ordered list {}", master.get_name()));
        return Ok(());
    }

    let mut target_record: Option<Arc<MainRecordImpl>> = None;
    let mut merged = Vec::new();
    for (l, list) in lists.iter().enumerate() {
        let (Some(target_list), Some(winning_list)) = (&list.target, &list.winning) else {
            continue;
        };
        if lists_equal(target_list, winning_list, false) {
            continue;
        }
        merged.push(MergedList {
            name: merge.lists[l].to_owned(),
            entries: target_list.items.len(),
            winning_entries: winning_list.items.len(),
        });
        let Some(target) = target else {
            continue;
        };
        let result = (|| -> Result<(), String> {
            if target_record.is_none() {
                let args = CopyArgs {
                    as_new: false,
                    deep_copy: true,
                    ..CopyArgs::default()
                };
                let source: ElementRef = winning.clone();
                let copied = copy_element_to_file(&source, target, &args)?
                    .and_then(|element| element.as_element_impl()?.main_record_impl())
                    .ok_or_else(|| "Access violation: the copy of the winning override is not a record".to_owned())?;
                target_record = Some(copied);
            }
            let record = target_record.as_ref().expect("copied above");
            record.remove_element_by_name(merge.lists[l]);
            for (_, entry) in &target_list.items {
                copy_element_to_record(entry, record, true, true)?;
            }
            // update counts
            if let Some(count_name) = merge.counts.get(l).filter(|name| !name.is_empty()) {
                record.add(count_name, true)?;
                if let Some(count) = record.get_element_by_path(count_name) {
                    match record.get_element_by_name(merge.lists[l]).filter(is_container) {
                        Some(entries) => {
                            let entries = entries.as_container().map_or(0, |entries| entries.get_element_count());
                            count.set_native_value(Variant::Int(i64::from(entries)))?;
                        }
                        None => count.remove(),
                    }
                }
            }
            Ok(())
        })();
        if let Err(message) = result {
            report.records.push(merged_record(master, &winning, merged));
            return Err(message);
        }
    }
    if !merged.is_empty() {
        report.records.push(merged_record(master, &winning, merged));
    }
    Ok(())
}

fn merged_record(master: &Arc<MainRecordImpl>, winning: &Arc<MainRecordImpl>, lists: Vec<MergedList>) -> MergedRecord {
    MergedRecord {
        name: master.get_name(),
        signature: master.get_signature(),
        form_id: master.get_load_order_form_id().to_cardinal(),
        winning_file: winning.get_file().map(|file| file.get_name()).unwrap_or_default(),
        lists,
    }
}

/// Delphi's `TStringList` as the handler uses it: strings with an element
/// each, compared with the locale (`CaseSensitive` off, `UseLocale` on),
/// `Duplicates` at its default, `dupIgnore`.
struct StringList {
    items: Vec<(String, ElementRef)>,
    sorted: bool,
}

impl StringList {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            sorted: false,
        }
    }

    /// `CompareStrings`: `AnsiCompareText`.
    fn compare(a: &str, b: &str) -> Ordering {
        ansi_compare_text(a, b)
    }

    /// `SetSorted`: a list that becomes sorted is sorted.
    fn set_sorted(&mut self, sorted: bool) {
        if sorted && !self.sorted {
            self.sort();
        }
        self.sorted = sorted;
    }

    /// `Sort` (`CustomSort` with `QuickSort`), only for a list that is not
    /// sorted already and has two strings or more.
    fn sort(&mut self) {
        if !self.sorted && self.items.len() > 1 {
            let last = self.items.len() - 1;
            self.quick_sort(0, last);
        }
    }

    /// `TStringList.QuickSort`, which is not stable: the order it leaves
    /// equal strings in decides which entry gets which count.
    fn quick_sort(&mut self, mut l: usize, r: usize) {
        loop {
            let mut i = l;
            let mut j = r as isize;
            let mut p = (l + r) >> 1;
            loop {
                while Self::compare(&self.items[i].0, &self.items[p].0) == Ordering::Less {
                    i += 1;
                }
                while Self::compare(&self.items[j as usize].0, &self.items[p].0) == Ordering::Greater {
                    j -= 1;
                }
                if i as isize <= j {
                    let ju = j as usize;
                    if i != ju {
                        self.items.swap(i, ju);
                    }
                    if p == i {
                        p = ju;
                    } else if p == ju {
                        p = i;
                    }
                    i += 1;
                    j -= 1;
                }
                if i as isize > j {
                    break;
                }
            }
            if (l as isize) < j {
                self.quick_sort(l, j as usize);
            }
            l = i;
            if i >= r {
                break;
            }
        }
    }

    /// `Find`: a search by halves, also in a list that is not sorted. With
    /// `dupIgnore` a match moves the search to the first equal string.
    fn find(&self, text: &str) -> (bool, usize) {
        let mut found = false;
        let mut low: isize = 0;
        let mut high: isize = self.items.len() as isize - 1;
        while low <= high {
            let middle = (low + high) >> 1;
            match Self::compare(&self.items[middle as usize].0, text) {
                Ordering::Less => low = middle + 1,
                ordering => {
                    high = middle - 1;
                    if ordering == Ordering::Equal {
                        found = true;
                        low = middle;
                    }
                }
            }
        }
        (found, low as usize)
    }

    /// `AddObject`: appended to a list that is not sorted; in a sorted list
    /// put in its place, unless an equal string is there (`dupIgnore`).
    fn add_object(&mut self, text: String, object: ElementRef) {
        if !self.sorted {
            self.items.push((text, object));
            return;
        }
        let (found, index) = self.find(&text);
        if found {
            return;
        }
        self.items.insert(index, (text, object));
    }

    fn delete(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }
}
