// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas

//! The sort of the responses of a topic (`TwbGroupRecord.Sort` of a group of
//! type 7 under `wbSortINFO`, with `ProcessDIAL` and `DoInsertRecord`) and
//! the `INOM` and `INOA` lists it gives the topic.
//!
//! Each `INFO` names the response it follows in `PNAM`. The sort puts the
//! responses of the topic and of its masters' versions (`INOM`, "Masters
//! only") or of every earlier version (`INOA`, "All previous modules") into
//! one linked list by following those links (`TwbMainRecordEntry`, one
//! entry per FormID), keeps the order in the `INOM` and `INOA` subrecords of
//! the topic (`dfDontSave`, internal edits), and orders the responses of the
//! group by the masters-only list. `TwbMainRecord.Init` sorts the group of a
//! topic that has no list yet, so the lists exist once a topic is built,
//! which is what the GUI shows and compares.
//!
//! Under `wbFillPNAM` (off in the GUI, on in the quick clean modes and with
//! `-FillPNAM`) a response without `PNAM` sorts its group when it is built,
//! and the sort gives every such response of the group the `PNAM` of the
//! response it follows (an internal edit).
//!
//! Not ported: the postponed sort of a group inside an update
//! (`gsSortPostponed`).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::interface::element::{Container, Element, ElementRef, File, MainRecord};
use crate::interface::globals::{
    begin_internal_edit, can_sort_info, display_load_order_form_id, end_internal_edit, fill_inoa, fill_inom, fill_pnam,
    is_skyrim, sort_info,
};
use crate::interface::misc::{Variant, progress};
use crate::interface::types::{FileState, Signature};

use super::{ElementImpl, GroupRecordImpl, MainRecordImpl};

/// The identity of a record in the list.
fn id(record: &MainRecordImpl) -> usize {
    std::ptr::from_ref(record) as usize
}

fn master_or_self(record: &Arc<MainRecordImpl>) -> Arc<MainRecordImpl> {
    record.master().unwrap_or_else(|| record.clone())
}

struct Node {
    record: Arc<MainRecordImpl>,
    prev: Option<usize>,
    next: Option<usize>,
}

/// Port of `TwbMainRecordEntryHeader` with the entries of the records
/// (`mrePrev`, `mreNext`, `mreGeneration`): a list that holds one version of
/// each FormID at most.
#[derive(Default)]
struct EntryList {
    nodes: HashMap<usize, Node>,
    head: Option<usize>,
    tail: Option<usize>,
}

impl EntryList {
    /// Port of `GetIsInList`.
    fn in_list(&self, record: &MainRecordImpl) -> bool {
        self.nodes.contains_key(&id(record))
    }

    /// Port of `RemoveEntryInternal`.
    fn remove_internal(&mut self, record: &MainRecordImpl) {
        let Some(node) = self.nodes.remove(&id(record)) else {
            return;
        };
        match node.prev {
            Some(prev) => {
                if let Some(prev) = self.nodes.get_mut(&prev) {
                    prev.next = node.next;
                }
            }
            None => self.head = node.next,
        }
        match node.next {
            Some(next) => {
                if let Some(next) = self.nodes.get_mut(&next) {
                    next.prev = node.prev;
                }
            }
            None => self.tail = node.prev,
        }
    }

    /// Port of `RemoveEntry`: every version of the FormID leaves the list.
    fn remove_entry(&mut self, record: &Arc<MainRecordImpl>) {
        let master = master_or_self(record);
        self.remove_internal(&master);
        for record in master.overrides() {
            self.remove_internal(&record);
        }
    }

    /// Port of `InsertEntryAfter`.
    fn insert_after(&mut self, record: &Arc<MainRecordImpl>, after: &MainRecordImpl) {
        self.remove_entry(record);
        let after = id(after);
        let Some(next) = self.nodes.get(&after).map(|node| node.next) else {
            // The entry to follow left with the record (a version of the
            // same FormID); upstream asserts here.
            self.insert_tail(record);
            return;
        };
        let own = id(record);
        self.nodes.insert(
            own,
            Node {
                record: record.clone(),
                prev: Some(after),
                next,
            },
        );
        if let Some(node) = self.nodes.get_mut(&after) {
            node.next = Some(own);
        }
        match next {
            Some(next) => {
                if let Some(node) = self.nodes.get_mut(&next) {
                    node.prev = Some(own);
                }
            }
            None => self.tail = Some(own),
        }
    }

    /// Port of `InsertEntryHead`.
    fn insert_head(&mut self, record: &Arc<MainRecordImpl>) {
        self.remove_entry(record);
        let own = id(record);
        let next = self.head;
        self.nodes.insert(
            own,
            Node {
                record: record.clone(),
                prev: None,
                next,
            },
        );
        match next {
            Some(next) => {
                if let Some(node) = self.nodes.get_mut(&next) {
                    node.prev = Some(own);
                }
            }
            None => self.tail = Some(own),
        }
        self.head = Some(own);
    }

    /// Port of `InsertEntryTail`.
    fn insert_tail(&mut self, record: &Arc<MainRecordImpl>) {
        self.remove_entry(record);
        let own = id(record);
        let prev = self.tail;
        self.nodes.insert(
            own,
            Node {
                record: record.clone(),
                prev,
                next: None,
            },
        );
        match prev {
            Some(prev) => {
                if let Some(node) = self.nodes.get_mut(&prev) {
                    node.next = Some(own);
                }
            }
            None => self.head = Some(own),
        }
        self.tail = Some(own);
    }
}

/// The exception that ends a sort: a cycle of `PNAM` links (`Abort`) or a
/// list that does not hold the responses of the group (an assertion).
struct SortAborted;

/// Port of `DoInsertRecord`: the response goes after the response its
/// `PNAM` links to, which goes into the list first; a response with an
/// empty `PNAM` goes to the head, one without `PNAM` to the tail.
fn do_insert_record(
    list: &mut EntryList,
    insert: &Arc<MainRecordImpl>,
    stack: &mut Vec<Arc<MainRecordImpl>>,
) -> Result<(), SortAborted> {
    let target = insert
        .get_element_links_to("PNAM")
        .and_then(|element| element.as_element_impl()?.main_record_impl());
    if let Some(mut target) = target {
        if !list.in_list(&target) {
            let master = master_or_self(&target);
            if let Some(last) = master.overrides().last() {
                target = last.clone();
            }
            if !list.in_list(&target) {
                target = master;
            }
        }
        if !list.in_list(&target) {
            if stack.iter().any(|entry| Arc::ptr_eq(entry, &target)) {
                // `ReportCycle`.
                progress(&format!(
                    "Cyclic PNAM references found for {} {}: ",
                    file_name(insert),
                    insert.get_name()
                ));
                for entry in stack.iter().rev() {
                    progress(&format!("referenced by {} {}: ", file_name(entry), entry.get_name()));
                    if Arc::ptr_eq(entry, &target) {
                        break;
                    }
                }
                return Err(SortAborted);
            }
            stack.push(insert.clone());
            let result = do_insert_record(list, &target, stack);
            stack.pop();
            result?;
        }
        list.insert_after(insert, &target);
    } else if insert.get_element_exists("PNAM") {
        list.insert_head(insert);
    } else {
        list.insert_tail(insert);
    }
    Ok(())
}

fn file_name(record: &MainRecordImpl) -> String {
    record.get_file().map(|file| file.get_name()).unwrap_or_default()
}

/// The quest of a response for the `PNAM` fill: `QSTI` of the response,
/// in Skyrim (which has no `QSTI`) `QNAM` of its topic.
fn info_quest(record: &MainRecordImpl) -> i64 {
    let value = if is_skyrim() {
        record
            .base
            .container()
            .and_then(|container| container.as_element_impl()?.group_record_impl())
            .and_then(|group| group.children_of())
            .map(|topic| topic.get_element_native_value("QNAM"))
    } else {
        Some(record.get_element_native_value("QSTI"))
    };
    value.and_then(|value| value.as_ordinal()).unwrap_or(0)
}

/// The `wbFillPNAM` part of `ProcessDIAL`: a response of the group without
/// `PNAM` follows the closest response before it in the list that is not
/// deleted and belongs to the same quest, or gets an empty `PNAM` when
/// there is none. The edit is internal.
fn fill_pnam_of(list: &EntryList, target: &Arc<MainRecordImpl>, prev: Option<usize>) {
    if target.get_element_exists("PNAM") {
        return;
    }
    let quest = info_quest(target);
    let mut insert = prev;
    while let Some(insert_id) = insert {
        let Some(node) = list.nodes.get(&insert_id) else { break };
        let candidate = &node.record;
        if !candidate.get_is_deleted() && info_quest(candidate) == quest {
            let form_id = candidate.get_load_order_form_id().to_cardinal();
            let set = match target.add("PNAM", false) {
                Ok(Some(element)) => element.set_native_value(Variant::UInt(u64::from(form_id))).is_ok(),
                _ => false,
            };
            if !set {
                target.remove_element_by_name("PNAM");
            }
            return;
        }
        insert = node.prev;
    }
    let _ = target.add("PNAM", false);
}

/// Port of `MasterRecordsFromMasterFilesAndSelf`: the versions of the record
/// up to itself that are in its own file or a master of it.
fn master_records_from_master_files_and_self(record: &Arc<MainRecordImpl>) -> Vec<Arc<MainRecordImpl>> {
    let Some(master) = record.master() else {
        return vec![record.clone()];
    };
    let Some(file) = record.get_file() else {
        return vec![record.clone()];
    };
    let masters: Vec<String> = (0..file.get_master_count(true))
        .filter_map(|index| file.get_master(index, true))
        .map(|file| file.get_name())
        .collect();
    let mut result = Vec::new();
    let mut add = |candidate: &Arc<MainRecordImpl>| -> bool {
        if Arc::ptr_eq(candidate, record) {
            result.push(candidate.clone());
            return true;
        }
        let candidate_file = candidate.get_file().map(|file| file.get_name()).unwrap_or_default();
        if candidate_file.eq_ignore_ascii_case(&file.get_name())
            || masters.iter().any(|name| name.eq_ignore_ascii_case(&candidate_file))
        {
            result.push(candidate.clone());
        }
        false
    };
    if !add(&master) {
        for candidate in master.overrides() {
            if add(&candidate) {
                break;
            }
        }
    }
    if result.is_empty() {
        vec![record.clone()]
    } else {
        result
    }
}

impl GroupRecordImpl {
    /// `TwbGroupRecord.Sort` without `aForce`, as the navigation tree calls
    /// it before it orders the responses of a topic by their sort order.
    pub fn sort_responses(&self) {
        self.sort();
    }

    /// Port of `TwbGroupRecord.Sort` for a group of the responses of a
    /// topic (type 7) when the game sorts them (`wbCanSortINFO`). Returns
    /// whether the group was such a group.
    pub(crate) fn sort_topic(&self, force: bool) -> bool {
        if !can_sort_info() || self.group_type() != 7 {
            return false;
        }
        let _ = self.sort_topic_inner(force, false);
        true
    }

    fn sort_topic_inner(&self, force: bool, nested: bool) -> Result<(), SortAborted> {
        if self.gr_sorting.load(Ordering::Relaxed) {
            return Ok(());
        }
        if !force && self.gr_sorted.load(Ordering::Relaxed) {
            return Ok(());
        }
        if self
            .file
            .upgrade()
            .is_some_and(|file| file.get_file_states().contains(FileState::fsScanning))
        {
            return Ok(());
        }
        if !sort_info() || !display_load_order_form_id() {
            return Ok(());
        }
        let Some(children_of) = self.children_of() else {
            return Ok(());
        };
        self.gr_sorting.store(true, Ordering::Relaxed);
        let mut result = Ok(());
        if fill_inoa() {
            result = self.process_dial(&children_of, false);
        }
        if result.is_ok() {
            result = self.process_dial(&children_of, true);
        }
        if result.is_err() {
            progress(&format!(
                "<Warning: could not sort INFO for [\"{}\" in \"{}\"] because of previous error>",
                self.get_name(),
                self.file.upgrade().map(|file| file.get_name()).unwrap_or_default()
            ));
        }
        self.gr_sorted.store(true, Ordering::Relaxed);
        self.gr_sorting.store(false, Ordering::Relaxed);
        // The outer sort of a group ends too, as the exception reaches it.
        if nested { result } else { Ok(()) }
    }

    /// Port of `ProcessDIAL`.
    fn process_dial(&self, children_of: &Arc<MainRecordImpl>, only_masters: bool) -> Result<(), SortAborted> {
        let records = if only_masters {
            master_records_from_master_files_and_self(children_of)
        } else {
            let master = master_or_self(children_of);
            let mut records = vec![master.clone()];
            if !Arc::ptr_eq(&master, children_of) {
                for record in master.overrides() {
                    let done = Arc::ptr_eq(&record, children_of);
                    records.push(record);
                    if done {
                        break;
                    }
                }
            }
            records
        };
        let groups: Vec<Arc<GroupRecordImpl>> = records
            .iter()
            .filter_map(|record| record.child_group())
            .filter(|group| group.container.element_count() > 0)
            .collect();
        for group in &groups {
            if !std::ptr::eq(Arc::as_ptr(group), self) {
                group.sort_topic_inner(false, true)?;
            }
        }

        let mut list = EntryList::default();
        for group in &groups {
            for element in group.container.elements() {
                if let Some(record) = element.as_element_impl().and_then(ElementImpl::main_record_impl) {
                    do_insert_record(&mut list, &record, &mut Vec::new())?;
                }
            }
        }

        let keep_list = (fill_inom() && only_masters) || (fill_inoa() && !only_masters);
        // From the tail: the records for the list of the topic, and the
        // records of the other groups out of the list.
        let mut listed: Vec<Arc<MainRecordImpl>> = Vec::new();
        let mut current = list.tail;
        while let Some(current_id) = current {
            let Some(node) = list.nodes.get(&current_id) else { break };
            let record = node.record.clone();
            let prev = node.prev;
            if keep_list {
                listed.push(record.clone());
            }
            let in_this_group = record
                .base
                .container()
                .and_then(|container| container.as_element_impl()?.group_record_impl())
                .is_some_and(|group| std::ptr::eq(Arc::as_ptr(&group), self));
            if !in_this_group {
                list.remove_entry(&record);
            } else if only_masters && fill_pnam() && !record.get_is_deleted() && begin_internal_edit(false) {
                fill_pnam_of(&list, &record, prev);
                end_internal_edit();
            }
            current = prev;
        }

        let mut new_elements: Vec<ElementRef> = Vec::new();
        if only_masters {
            if list.nodes.len() != self.container.element_count() {
                // `Assert(mreHeader.mrehCount = Length(cntElements))`.
                return Err(SortAborted);
            }
            let mut order = Vec::with_capacity(list.nodes.len());
            let mut current = list.tail;
            while let Some(current_id) = current {
                let node = &list.nodes[&current_id];
                order.push(node.record.clone());
                current = node.prev;
            }
            order.reverse();
            for (index, record) in order.iter().enumerate() {
                record.set_sort_order(index as i32);
                new_elements.push(record.clone() as ElementRef);
            }
        }

        if keep_list {
            let signature = Signature::new(if only_masters { b"INOM" } else { b"INOA" });
            if begin_internal_edit(false) {
                if let Some(existing) = children_of.get_record_by_signature(signature) {
                    existing.remove();
                }
                if let Ok(Some(list_element)) = children_of.add(&signature.to_string(), false)
                    && let Some(container) = list_element.as_container()
                {
                    list_element.begin_update();
                    for record in listed.iter().rev() {
                        if let Ok(Some(entry)) = container.add("", false) {
                            let form_id = record.get_load_order_form_id().to_cardinal();
                            let _ = entry.set_native_value(Variant::UInt(u64::from(form_id)));
                        }
                    }
                    list_element.end_update();
                }
                end_internal_edit();
            }
        }

        if only_masters {
            let mut elements = self.container.cnt_elements.write().unwrap();
            *elements = new_elements;
        }
        Ok(())
    }
}

impl MainRecordImpl {
    /// The end of `TwbMainRecord.Init` under `wbSortINFO`: a topic without
    /// its lists sorts the group of its responses, which makes them.
    pub(crate) fn sort_info_after_init(self: &Arc<Self>) {
        if !(can_sort_info() && sort_info()) || self.get_is_deleted() || self.get_is_partial_form() {
            return;
        }
        let signature = self.get_signature();
        let is_info = signature == Signature::new(b"INFO");
        if !is_info && signature != Signature::new(b"DIAL") {
            return;
        }
        if !begin_internal_edit(false) {
            return;
        }
        if fill_pnam() && is_info && self.get_record_by_signature(Signature::new(b"PNAM")).is_none() {
            // A response without `PNAM` sorts its group, which fills it.
            if let Some(group) = self
                .base
                .container()
                .and_then(|container| container.as_element_impl()?.group_record_impl())
            {
                group.sort_topic(true);
            }
        } else if !is_info {
            let missing = (fill_inom() && self.get_record_by_signature(Signature::new(b"INOM")).is_none())
                || (fill_inoa() && self.get_record_by_signature(Signature::new(b"INOA")).is_none());
            if missing && let Some(group) = self.child_group() {
                group.sort_topic(true);
            }
        }
        end_internal_edit();
    }
}
