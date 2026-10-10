// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbImplementation.pas (TwbElement.Reached,
// TwbContainer.Reached, TwbMainRecord.Reached, TwbGroupRecord.Reached,
// TwbFile.Reached, TwbElement.ResetReachable, TwbContainer.ResetReachable,
// TwbMainRecord.ResetReachable, TwbFile.BuildReachable,
// TwbElement.GetIsReachable, TwbElement.GetIsNotReachable,
// TwbMainRecord.GetIsReachable, TwbMainRecord.GetIsNotReachable,
// TwbElement.GetNoReach, TwbElement.LinksToParent, TwbMainRecord.LinksToParent,
// TwbGroupRecord.LinksToParent)

//! "Build Reachable Info" (`mniNavBuildReachableClick`): which records a
//! game reaches while it runs.
//!
//! [`build_reachable`] is `TwbFile.BuildReachable`: it marks the file header
//! record, the hardcoded records and the records of the groups a game always
//! loads (the "always reached" signatures and the special cases per game)
//! and lets [`reached`] walk from them: an element reaches the record it
//! links to (unless its definition is `wb..NoReach`) and its parent element,
//! a group reaches the record of its label, a record reaches its master or
//! its overrides and its child group, and the scripting, quest-objective,
//! furniture and NPC cases reach the records that refer to them. What is
//! left is "not reachable", which the filter of the navigation tree can show
//! (`FilterByNotReachableStatus`).
//!
//! The states are `esReachable` and `esNotReachable` on the elements, as
//! upstream's `eStates`. `SetContainer` hands `esNotReachable` to a new
//! child of a container that has it ([`set_container`]); a main record keeps
//! its own state and no child states (`TwbMainRecord.ResetReachable` does
//! not walk its elements), which is why a record has to be built again to
//! see the states of its elements.
//!
//! The collector of `TwbMainRecord.Reached` (`mrcMainRecords`, which keeps
//! the records a call has to process after its own body so that a cycle of
//! references does not recurse forever) is a thread local here, as the
//! `threadvar`-like process-wide variable upstream; a build runs on one
//! thread. The collector queues a record whose body is not the one running,
//! and the walk enters every record once ([`ENTERED`], which upstream has in
//! the `esReachable` mark): a record a definition hides and a non-winning
//! override never take that mark upstream, and a cycle of references through
//! such records would be walked again and again.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::Arc;

use crate::interface::def::Def;
use crate::interface::element::{Container, Element, ElementRef, MainRecord, MainRecordRef};
use crate::interface::globals::{GameMode, game_mode};
use crate::interface::types::{ElementType, Signature};

use super::refs::can_contain_form_ids;
use super::write::ElementState;
use super::{ElementBase, ElementImpl, FileImpl, GroupRecordImpl, MainRecordImpl};

/// `esReachable in eStates`.
pub(crate) fn is_reachable_state(base: &ElementBase) -> bool {
    base.has_state(ElementState::esReachable)
}

/// `esNotReachable in eStates`.
pub(crate) fn is_not_reachable_state(base: &ElementBase) -> bool {
    base.has_state(ElementState::esNotReachable)
}

/// Port of `TwbElement.GetNoReach`: from the value definition, else the
/// definition.
pub(crate) fn no_reach(element: &dyn ElementImpl) -> bool {
    let def: Option<Arc<dyn Def>> = match element.get_value_def() {
        Some(def) => Some(def),
        None => element.get_def().map(|def| def as Arc<dyn Def>),
    };
    def.is_some_and(|def| def.get_no_reach())
}

/// Port of `TwbElement.SetContainer`: a new child of a container that is
/// not reachable is not reachable either.
pub(crate) fn set_container(container: &ElementBase, element: &ElementBase) {
    if is_not_reachable_state(container) {
        element.include_state(ElementState::esNotReachable);
    }
}

fn children(element: &dyn ElementImpl) -> Vec<ElementRef> {
    if let Some(container) = element.as_container() {
        // `DoInit`.
        container.get_element_count();
    }
    element.container_base().map(|base| base.elements()).unwrap_or_default()
}

/// Port of `IwbElementInternal.LinksToParent` of the element classes:
/// `TwbElement.LinksToParent` is false, the main record and group record
/// overrides follow.
pub(crate) fn element_links_to_parent(element: &dyn ElementImpl) -> bool {
    if let Some(record) = element.main_record_impl() {
        return main_record_links_to_parent(&record);
    }
    element
        .group_record_impl()
        .is_some_and(|group| group_links_to_parent(&group))
}

/// Port of `TwbMainRecord.LinksToParent`.
fn main_record_links_to_parent(record: &Arc<MainRecordImpl>) -> bool {
    match &record.get_signature().0 {
        b"CELL" => {
            let data = record.get_element_by_path("DATA");
            match data {
                Some(data) => data.get_edit_value().as_bytes().first() != Some(&b'1'),
                None => true,
            }
        }
        b"INFO" | b"REFR" | b"PGRE" | b"PMIS" | b"ACHR" | b"ACRE" | b"PGRD" | b"PARW" | b"PBEA" | b"PFLA" | b"PCON"
        | b"PBAR" | b"PHZD" | b"NAVM" | b"ROAD" | b"LAND" => true,
        b"DLBR" | b"DIAL" | b"SCEN" => crate::interface::globals::vwd_as_quest_children(),
        _ => false,
    }
}

/// Port of `TwbGroupRecord.LinksToParent`: the groups that show below their
/// record.
fn group_links_to_parent(group: &GroupRecordImpl) -> bool {
    matches!(group.group_type(), 4 | 5 | 8..=10)
}

/// The element at `path` of a record, as its `LinksTo` main record.
fn element_links_to(record: &Arc<MainRecordImpl>, path: &str) -> Option<Arc<MainRecordImpl>> {
    record
        .get_element_by_path(path)?
        .get_links_to()?
        .as_element_impl()?
        .main_record_impl()
}

/// The file an element belongs to.
fn file_of(element: &dyn ElementImpl) -> Option<Arc<FileImpl>> {
    let mut current = element.self_element_ref();
    while let Some(element) = current {
        let impl_ = element.as_element_impl()?;
        if let Some(file) = impl_.file_impl() {
            return Some(file);
        }
        current = impl_.element_base().container();
    }
    None
}

/// `Element.SortKey[aExtended]` of the element at `path`, empty without one.
fn sort_key_of_path(record: &Arc<MainRecordImpl>, path: &str) -> String {
    match record.get_element_by_path(path) {
        Some(element) => element.get_sort_key(false),
        None => String::new(),
    }
}

// ----- Reached -----

thread_local! {
    /// Port of `_Collector`: the records `TwbMainRecord.Reached` queues
    /// while another record is processed.
    static COLLECTOR: RefCell<Vec<Arc<MainRecordImpl>>> = const { RefCell::new(Vec::new()) };
    /// Whether a call is processing (a collector exists).
    static COLLECTING: Cell<bool> = const { Cell::new(false) };
    /// Port of `_IgnoreCollector`: the next record is processed even while a
    /// call is running.
    static IGNORE_COLLECTOR: Cell<bool> = const { Cell::new(false) };
    /// The records whose body the walk ran, by address.
    ///
    /// Upstream has this in `esReachable`, which a record a definition
    /// hides (`GetDontShow`) and a non-winning override never take: a record
    /// of either kind is entered every time a reference reaches it again,
    /// and the walk of the five Skyrim SE masters cycles through the
    /// material and impact records until the stack of the worker is gone.
    /// The set stops the second visit; the parts a build marks are the same
    /// either way, because a second visit only repeats what the first did.
    static ENTERED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// Clears the records the walk entered, as `ResetReachable` clears
/// `esReachable`: `TwbFile.BuildReachable` of the files after the first one
/// sees what the walk before it reached.
pub fn begin_reachable_walk() {
    ENTERED.with_borrow_mut(HashSet::clear);
}

/// Port of `IwbElementInternal.Reached` of the element classes.
pub fn reached(element: &ElementRef) -> bool {
    match element.as_element_impl() {
        Some(impl_) => reached_impl(impl_),
        None => false,
    }
}

/// [`reached`] for a `WinningOverride` (`IwbMainRecord`).
fn reached_element_ref(record: &MainRecordRef) -> bool {
    match record.as_element_impl() {
        Some(impl_) => reached_impl(impl_),
        None => false,
    }
}

/// [`reached`] for an element object.
pub fn reached_impl(impl_: &dyn ElementImpl) -> bool {
    if let Some(record) = impl_.main_record_impl() {
        return reached_main_record(&record);
    }
    if let Some(group) = impl_.group_record_impl() {
        return reached_group(&group);
    }
    reached_container(impl_)
}

/// Port of `TwbElement.Reached`.
fn reached_element(impl_: &dyn ElementImpl) -> bool {
    let base = impl_.element_base();
    let result = !is_reachable_state(base);

    if impl_.get_dont_show() {
        return result;
    }

    base.exclude_state(ElementState::esNotReachable);
    base.include_state(ElementState::esReachable);

    if result {
        if !no_reach(impl_)
            && let Some(record) = impl_
                .as_dyn_element_impl()
                .and_then(|element| element.get_links_to())
                .and_then(|element| element.as_element_impl().and_then(|impl_| impl_.main_record_impl()))
        {
            let winning = record.get_winning_override();
            reached_element_ref(&winning);
        }
        if element_links_to_parent(impl_)
            && let Some(container) = base.container()
        {
            reached(&container);
        }
    }
    result
}

/// Port of `TwbContainer.Reached`.
fn reached_container(impl_: &dyn ElementImpl) -> bool {
    if impl_.get_dont_show() {
        return false;
    }
    if !reached_element(impl_) {
        return false;
    }
    for child in children(impl_) {
        if let Some(child_impl) = child.as_element_impl()
            && can_contain_form_ids(child_impl)
        {
            reached(&child);
        }
    }
    true
}

/// Port of `TwbMainRecord.Reached`.
fn reached_main_record(record: &Arc<MainRecordImpl>) -> bool {
    let base = record.element_base();
    if is_reachable_state(base) {
        return false;
    }
    if IGNORE_COLLECTOR.with(Cell::get) {
        IGNORE_COLLECTOR.with(|flag| flag.set(false));
    } else if COLLECTING.with(Cell::get) {
        COLLECTOR.with_borrow_mut(|queue| queue.push(record.clone()));
        return false;
    }
    // The body runs once per walk ([`ENTERED`]); upstream takes the same
    // mark in `esReachable` for the records it does not hide.
    if !ENTERED.with_borrow_mut(|entered| entered.insert(Arc::as_ptr(record) as usize)) {
        return false;
    }
    let top_level = !COLLECTING.with(Cell::get);
    if top_level {
        COLLECTING.with(|flag| flag.set(true));
    }

    let result = reached_main_record_body(record);

    // `while i <= High(Collector.mrcMainRecords)`.
    let mut index = 0;
    loop {
        let next = COLLECTOR.with_borrow(|queue| queue.get(index).cloned());
        match next {
            Some(record) => {
                IGNORE_COLLECTOR.with(|flag| flag.set(true));
                reached_main_record(&record);
                index += 1;
            }
            None => break,
        }
    }
    if top_level {
        COLLECTING.with(|flag| flag.set(false));
        COLLECTOR.with_borrow_mut(Vec::clear);
    }
    result
}

fn reached_main_record_body(record: &Arc<MainRecordImpl>) -> bool {
    let signature = record.get_signature();
    let is_complex = matches!(&signature.0, b"DIAL" | b"WRLD" | b"CELL" | b"DOBJ");
    if !(record.is_winning_override() || is_complex) {
        let winning = record.get_winning_override();
        return reached_element_ref(&winning);
    }
    let result = reached_container(record.as_element_impl().expect("a main record is an element"));
    if !result {
        return false;
    }
    if record.element_base().container().is_none() {
        return true;
    }
    let master_or_self = record.master_or_self_impl();
    match &signature.0 {
        b"SMBN" | b"SMQN" | b"SMEN" => {
            for ref_record in master_or_self.referenced_by() {
                if matches!(&ref_record.get_signature().0, b"SMBN" | b"SMQN" | b"SMEN")
                    && let Some(target) = element_links_to(&ref_record, "PNAM")
                    && target.get_load_order_form_id() == record.get_load_order_form_id()
                {
                    reached_main_record(&ref_record);
                }
            }
        }
        b"FURN" => {
            if game_mode() as u8 >= GameMode::gmTES5 as u8
                && record
                    .get_element_native_value("WBDT\\Bench Type")
                    .as_ordinal()
                    .unwrap_or(0)
                    > 0
                && let Some(keywords) = record.get_element_by_path("KWDA - Keywords")
                && let Some(keywords) = keywords.as_container()
            {
                for index in 0..keywords.get_element_count() {
                    let Some(keyword) = keywords.get_element(index) else {
                        continue;
                    };
                    let Some(base) = keyword
                        .get_links_to()
                        .and_then(|element| element.as_element_impl().and_then(|impl_| impl_.main_record_impl()))
                    else {
                        continue;
                    };
                    let master = base.master_or_self_impl();
                    for ref_record in master.referenced_by() {
                        if ref_record.get_signature().0 == *b"COBJ"
                            && let Some(target) = element_links_to(&ref_record, "BNAM")
                            && target.get_load_order_form_id() == master.get_load_order_form_id()
                        {
                            reached_main_record(&ref_record);
                        }
                    }
                }
            }
        }
        b"NPC_" => {
            if game_mode() as u8 >= GameMode::gmTES5 as u8 {
                for ref_record in master_or_self.referenced_by() {
                    if ref_record.get_signature().0 == *b"RELA" {
                        reached_main_record(&ref_record);
                    }
                }
            }
        }
        // `Master.IsReachable` of the quest's master.
        b"QUST" if game_mode() as u8 >= GameMode::gmTES5 as u8 => {
            let master_reachable = Element::get_is_reachable(&*master_or_self);
            for ref_record in master_or_self.referenced_by() {
                match &ref_record.get_signature().0 {
                    b"SCEN" => {
                        if element_links_to(&ref_record, "PNAM").is_some() && master_reachable {
                            reached_main_record(&ref_record);
                        }
                    }
                    b"DLBR" | b"DIAL" => {
                        if let Some(target) = element_links_to(&ref_record, "QNAM")
                            && target.get_load_order_form_id() == record.get_load_order_form_id()
                            && master_reachable
                        {
                            reached_main_record(&ref_record);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    if !main_record_links_to_parent(record) && is_complex {
        match record.master() {
            Some(master) => {
                reached_main_record(&master);
            }
            None => {
                for over in record.overrides() {
                    reached_main_record(&over);
                }
            }
        }
        if let Some(group) = record.child_group() {
            reached_impl(group.as_element_impl().expect("a group record is an element"));
        }
    }
    true
}

/// Port of `TwbGroupRecord.Reached`.
fn reached_group(group: &Arc<GroupRecordImpl>) -> bool {
    let base = group.element_base();
    if is_reachable_state(base) {
        return false;
    }
    if matches!(group.group_type(), 0 | 2 | 3) {
        return false;
    }
    if !reached_container(group.as_element_impl().expect("a group record is an element")) {
        return false;
    }
    if matches!(group.group_type(), 1 | 6..=10)
        && let Some(file) = file_of(group.as_element_impl().expect("a group record is an element"))
        && let Some(record) = file.record_by_form_id(
            crate::interface::form_id::FormID::from_cardinal(group.get_group_label()),
            false,
            group.get_masters_updated(),
        )
    {
        reached_main_record(&record);
    }
    true
}

// ----- ResetReachable -----

/// Port of `TwbElement.ResetReachable`, `TwbContainer.ResetReachable` and
/// `TwbMainRecord.ResetReachable` (which does not walk its elements).
pub fn reset_reachable(element: &dyn ElementImpl) {
    element.element_base().include_state(ElementState::esNotReachable);
    element.element_base().exclude_state(ElementState::esReachable);
    if element.main_record_impl().is_some() || element.get_element_type() == ElementType::etMainRecord {
        return;
    }
    for child in children(element) {
        if let Some(child) = child.as_element_impl() {
            reset_reachable(child);
        }
    }
}

// ----- GetIsReachable / GetIsNotReachable -----

/// Port of `IwbElementInternal.IsReachable` of the element classes.
pub(crate) fn element_is_reachable(element: &dyn ElementImpl) -> bool {
    match element.main_record_impl() {
        Some(record) => main_record_is_reachable(&record),
        None => is_reachable_state(element.element_base()),
    }
}

/// Port of `IwbElementInternal.IsNotReachable` of the element classes.
pub(crate) fn element_is_not_reachable(element: &dyn ElementImpl) -> bool {
    match element.main_record_impl() {
        Some(record) => main_record_is_not_reachable(&record),
        None => is_not_reachable_state(element.element_base()),
    }
}

/// Port of `TwbMainRecord.GetIsReachable`: the master's state, and its own
/// with the one of every override.
fn main_record_is_reachable(record: &Arc<MainRecordImpl>) -> bool {
    if let Some(master) = record.master() {
        return main_record_is_reachable(&master);
    }
    if !is_reachable_state(record.element_base()) {
        return false;
    }
    record
        .overrides()
        .iter()
        .all(|over| is_reachable_state(over.element_base()))
}

/// Port of `TwbMainRecord.GetIsNotReachable`: the master's state, and its
/// own with the one of every override.
fn main_record_is_not_reachable(record: &Arc<MainRecordImpl>) -> bool {
    if let Some(master) = record.master() {
        return main_record_is_not_reachable(&master);
    }
    if !is_not_reachable_state(record.element_base()) {
        return false;
    }
    record
        .overrides()
        .iter()
        .all(|over| is_not_reachable_state(over.element_base()))
}

// ----- TwbFile.BuildReachable -----

/// The groups whose records `TwbFile.BuildReachable` marks as reached
/// whatever refers to them.
const ALWAYS_REACHED_GROUPS: [&[u8; 4]; 14] = [
    b"ADDN", b"ANIO", b"AVIF", b"BSGN", b"CAMS", b"COBJ", b"CPTH", b"DFOB", b"DLVW", b"DOBJ", b"GMST", b"IDLE",
    b"LSCR", b"NAVI",
];

/// Port of `TwbFile.BuildReachable`: the entry points a game always loads,
/// then [`reached`] from them.
pub fn build_reachable(file: &Arc<FileImpl>) {
    let Some(container) = file.as_container() else {
        return;
    };
    if container.get_element_count() < 1 {
        return;
    }
    // `cntElements[0].Reached`: the file header record.
    if let Some(header) = container.get_element(0) {
        reached(&header);
    }
    for record in file.records() {
        if !record.mr_struct().form_id.is_hardcoded() {
            break;
        }
        if record.is_winning_override() {
            reached_main_record(&record);
        }
    }
    let mut signatures: Vec<Signature> = ALWAYS_REACHED_GROUPS
        .iter()
        .map(|signature| Signature::new(signature))
        .collect();
    signatures.push(Signature::new(b"RADS"));
    signatures.push(Signature::new(b"SKIL"));
    for signature in signatures {
        // Every record of the group, whatever override it is.
        reached_group_records_all(file, signature);
    }

    if crate::interface::globals::is_oblivion() || crate::interface::globals::is_fallout3() {
        reached_group_records(file, Signature::new(b"CLAS"), |record| {
            record.get_element_edit_value("DATA\\Flags").as_bytes().first() == Some(&b'1')
        });
    }
    if crate::interface::globals::is_fallout3() {
        reached_group_records(file, Signature::new(b"DIAL"), |record| {
            sort_key_of_path(record, "DATA\\Flags").as_bytes().get(1) == Some(&b'1')
        });
    }
    if crate::interface::globals::is_oblivion() || crate::interface::globals::is_fallout3() {
        reached_group_records(file, Signature::new(b"EYES"), |record| {
            sort_key_of_path(record, "DATA").as_bytes().first() == Some(&b'1')
        });
    }
    reached_group_records(file, Signature::new(b"HDPT"), |record| {
        if crate::interface::globals::is_fallout3() {
            return record
                .get_element_native_value("DATA\\Playable")
                .as_ordinal()
                .unwrap_or(0)
                != 0;
        }
        sort_key_of_path(record, "DATA").as_bytes().first() == Some(&b'1')
    });
    reached_group_records(file, Signature::new(b"PERK"), |record| {
        if crate::interface::globals::is_starfield() {
            return sort_key_of_path(record, "DATA\\Flags").as_bytes().get(1) == Some(&b'1');
        }
        record
            .get_element_native_value("DATA\\Playable")
            .as_ordinal()
            .unwrap_or(0)
            != 0
    });
    if !crate::interface::globals::is_oblivion() || crate::interface::globals::is_morrowind() {
        reached_group_records(file, Signature::new(b"NPC_"), |record| {
            record.get_element_edit_value("ACBS\\Flags").as_bytes().get(2) == Some(&b'1')
        });
    }
    reached_group_records(file, Signature::new(b"QUST"), |record| {
        let path = if game_mode() as u8 >= GameMode::gmTES5 as u8 {
            "DNAM"
        } else {
            "DATA"
        };
        let Some(data) = record.get_element_by_path(path) else {
            return false;
        };
        let Some(first) = data.as_container().and_then(|container| container.get_element(0)) else {
            return false;
        };
        first.get_edit_value().as_bytes().first() == Some(&b'1')
    });
    reached_group_records(file, Signature::new(b"RACE"), |record| {
        let path = if game_mode() as u8 >= GameMode::gmSF1 as u8 {
            "DAT2"
        } else {
            "DATA"
        };
        if record.get_element_by_path(path).is_none() {
            return false;
        }
        if crate::interface::globals::is_oblivion() {
            return record
                .get_element_native_value(&format!("{path}\\Playable"))
                .as_ordinal()
                .unwrap_or(0)
                != 0;
        }
        record
            .get_element_edit_value(&format!("{path}\\Flags"))
            .as_bytes()
            .first()
            == Some(&b'1')
    });
}

/// The records of the group of a signature that a condition picks, of the
/// winning overrides (`Rec.IsWinningOverride`, as the special cases of
/// `TwbFile.BuildReachable` read them).
fn reached_group_records(file: &Arc<FileImpl>, signature: Signature, condition: impl Fn(&Arc<MainRecordImpl>) -> bool) {
    let Some(group) = file.group_by_signature(signature) else {
        return;
    };
    for record in children(group.as_element_impl().expect("a group record is an element")) {
        let Some(record) = record.as_element_impl().and_then(|impl_| impl_.main_record_impl()) else {
            continue;
        };
        if record.is_winning_override() && condition(&record) {
            reached_main_record(&record);
        }
    }
}

/// Every record of the group of a signature, whatever override it is
/// (`for i := 0 to Pred(Group.ElementCount) do Group.Elements[i].Reached`).
fn reached_group_records_all(file: &Arc<FileImpl>, signature: Signature) {
    let Some(group) = file.group_by_signature(signature) else {
        return;
    };
    for element in children(group.as_element_impl().expect("a group record is an element")) {
        reached(&element);
    }
}
