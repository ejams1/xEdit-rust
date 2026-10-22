// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The navigation tree over a real plugin: `Update.esm` of the installed
//! Skyrim Special Edition (`XEDIT_SSE_DATA`), the grouping rules of
//! `vstNavInitNode`, the cell texts of `vstNavGetText`, the filtered tree of
//! `filter.apply` and the add list of `Container.GetAddList`. The tests do
//! nothing when `XEDIT_SSE_DATA` is not in the environment.

use std::path::Path;

use serde_json::json;
use xedit_core::implementation::ElementImpl;
use xedit_core::interface::globals::test_lock;
use xedit_core::interface::types::ElementType;
use xedit_gui::nav_tree::{NavSource, add_list_of};
use xedit_gui::widgets::virtual_tree::{TreeRow, TreeSource, TreeState};
use xedit_session::{Registry, Session};

/// Loads `Update.esm` of the SSE install, or returns `None` without it.
fn load_update() -> Option<(Registry, Session, NavSource)> {
    let data = std::env::var("XEDIT_SSE_DATA").ok()?;
    let path = Path::new(&data).join("Update.esm");
    if !path.exists() {
        return None;
    }
    xedit_core::implementation::clear_files_map();
    xedit_core::implementation::reset_load_order_slots();
    xedit_core::interface::clear_files();
    let mut session = Session::load("sse", &[path.to_string_lossy().into_owned()]).ok()?;
    session.allow_edit(true);
    let nav = NavSource::from_loaded();
    Some((Registry::standard(), session, nav))
}

fn row_text(nav: &NavSource, row: usize, column: usize) -> String {
    let mut out = TreeRow::default();
    nav.row(row, &mut out);
    out.cells.get(column).map(|cell| cell.text.clone()).unwrap_or_default()
}

fn row_element(nav: &NavSource, row: usize) -> Option<xedit_core::interface::ElementRef> {
    let node = nav.row_node(row)?;
    nav.node(node)?.element.clone()
}

fn signature_of(element: &xedit_core::interface::ElementRef) -> Option<String> {
    element
        .as_element_impl()
        .and_then(ElementImpl::main_record_impl)
        .map(|record| record.get_signature().to_string())
}

#[test]
fn the_file_root_shows_its_header_groups_and_records() {
    let _guard = test_lock();
    let Some((_registry, _session, mut nav)) = load_update() else {
        return;
    };
    nav.prepare(None);
    // The roots are the loaded files in load order.
    let names: Vec<String> = nav
        .roots
        .iter()
        .filter_map(|&id| nav.node(id)?.element.as_ref())
        .map(|element| element.get_name())
        .collect();
    assert!(names.contains(&"Skyrim.esm".to_owned()), "{names:?}");
    assert!(names.contains(&"Update.esm".to_owned()), "{names:?}");
    let orders: Vec<i32> = nav
        .roots
        .iter()
        .filter_map(|&id| nav.node(id)?.element.as_ref())
        .filter_map(|element| {
            element
                .as_element_impl()
                .and_then(ElementImpl::file_impl)
                .map(|file| file.load_order())
        })
        .collect();
    assert_eq!(orders.len(), names.len());
    assert!(orders.windows(2).all(|pair| pair[0] <= pair[1]), "{orders:?}");
    // The selection of the first root.
    let mut state = TreeState::new(vec![201.0, 125.0, 125.0]);
    state.select_only(0);
    let selection = nav.selection(&state);
    assert_eq!(selection.nodes.len(), 1);
    assert!(selection.nodes[0].is_file(), "{:?}", selection.nodes[0]);
    assert_eq!(selection.files(), vec![selection.nodes[0].element_name.clone()]);
    assert!(selection.records().is_empty());
    // `vstNavInitChildren`: the file's header record and its groups.
    let update = nav
        .roots
        .iter()
        .position(|&id| {
            nav.node(id)
                .and_then(|node| node.element.as_ref())
                .is_some_and(|element| element.get_name() == "Update.esm")
        })
        .expect("Update.esm is a root");
    assert!(TreeSource::toggle(&mut nav, update));
    nav.prepare(None);
    let header = row_text(&nav, update + 1, 0);
    assert_eq!(header, "File Header");
    let mut groups = Vec::new();
    for row in update + 1..nav.row_count() {
        let Some(element) = row_element(&nav, row) else {
            continue;
        };
        if element.get_element_type() == ElementType::etGroupRecord {
            groups.push(row_text(&nav, row, 0));
        }
    }
    assert!(groups.iter().any(|name| name == "Weapon"), "{groups:?}");
    assert!(groups.iter().any(|name| name == "Cell"), "{groups:?}");
    // `vstNavInitNode`/`vstNavGetText` below a group.
    let first_group = (update + 1..nav.row_count())
        .find(|&row| {
            row_element(&nav, row).is_some_and(|element| element.get_element_type() == ElementType::etGroupRecord)
        })
        .expect("a group");
    TreeSource::toggle(&mut nav, first_group);
    nav.prepare(None);
    let first_record = first_group + 1;
    let text = row_text(&nav, first_record, 0);
    // `LoadOrderFormID.ToString(True)`: eight hex digits (the pairs of
    // `wbPrettyFormID` are off in 4.1.5q by default).
    assert_eq!(text.len(), 8, "{text}");
    assert!(text.chars().all(|c| c.is_ascii_hexdigit()), "{text}");
    assert!(!row_text(&nav, first_record, 2).is_empty(), "the display name");
}

#[test]
fn a_parented_group_shows_below_its_record() {
    let _guard = test_lock();
    let Some((_registry, _session, mut nav)) = load_update() else {
        return;
    };
    nav.prepare(None);
    let update = nav
        .roots
        .iter()
        .position(|&id| {
            nav.node(id)
                .and_then(|node| node.element.as_ref())
                .is_some_and(|element| element.get_name() == "Update.esm")
        })
        .expect("Update.esm is a root");
    TreeSource::toggle(&mut nav, update);
    nav.prepare(None);
    // Expand groups until a CELL with a child group (placed references) is
    // found.
    let mut found = false;
    let mut row = update + 1;
    while row < nav.row_count() && !found {
        let Some(element) = row_element(&nav, row) else {
            row += 1;
            continue;
        };
        if element.get_element_type() == ElementType::etGroupRecord && TreeSource::toggle(&mut nav, row) {
            nav.prepare(None);
            // The records of the group follow; look for a CELL record whose
            // node has children.
            let mut child = row + 1;
            while child < nav.row_count() {
                let Some(node_id) = nav.row_node(child) else { break };
                let Some(node) = nav.node(node_id) else { break };
                if node.depth <= 1 {
                    break;
                }
                if node.has_children
                    && node
                        .element
                        .as_ref()
                        .is_some_and(|element| signature_of(element).as_deref() == Some("CELL"))
                {
                    found = true;
                    break;
                }
                child += 1;
            }
        }
        row += 1;
    }
    assert!(found, "a CELL with a child group is reachable");
}

#[test]
fn the_selection_carries_the_records_and_their_files_on_a_record() {
    let _guard = test_lock();
    let Some((_registry, _session, mut nav)) = load_update() else {
        return;
    };
    nav.prepare(None);
    // The file root and its first group, then a record below it.
    TreeSource::toggle(&mut nav, 0);
    nav.prepare(None);
    let group = (1..nav.row_count())
        .find(|&row| {
            row_element(&nav, row).is_some_and(|element| element.get_element_type() == ElementType::etGroupRecord)
        })
        .expect("a group");
    TreeSource::toggle(&mut nav, group);
    nav.prepare(None);
    let record_row = (group + 1..nav.row_count())
        .find(|&row| signature_of(&row_element(&nav, row).expect("an element")).is_some())
        .expect("a record");
    let mut state = TreeState::new(vec![201.0, 125.0, 125.0]);
    state.select_only(record_row);
    let selection = nav.selection(&state);
    assert_eq!(selection.nodes.len(), 1);
    let node = &selection.nodes[0];
    assert!(node.is_record(), "{node:?}");
    assert!(node.form_id.is_some() && node.file.is_some());
    assert_eq!(selection.records().len(), 1);
}

#[test]
fn the_add_list_names_the_groups_of_a_file() {
    let _guard = test_lock();
    let Some((_registry, _session, nav)) = load_update() else {
        return;
    };
    let update = nav
        .roots
        .iter()
        .find(|&&id| {
            nav.node(id)
                .and_then(|node| node.element.as_ref())
                .is_some_and(|element| element.get_name() == "Update.esm")
        })
        .expect("Update.esm is a root");
    let element = nav.node(*update).and_then(|node| node.element.clone()).unwrap();
    let list = add_list_of(&element);
    assert!(list.iter().any(|item| item.starts_with("WOOP - ")), "{list:?}");
    assert!(
        list.iter().all(|item| !item.starts_with("WEAP - ")),
        "Update.esm already has a WEAP group: {list:?}"
    );
}

#[test]
fn the_filter_view_keeps_the_surviving_nodes_only() {
    let _guard = test_lock();
    let Some((registry, mut session, mut nav)) = load_update() else {
        return;
    };
    let value = registry
        .call(
            &mut session,
            "filter.apply",
            json!({ "options": { "by_signature": "CELL" } }),
        )
        .expect("filter.apply");
    let response: xedit_session::filter::FilterApplyResponse = serde_json::from_value(value).expect("the response");
    assert!(!response.nodes.is_empty());
    let statuses: Vec<(
        usize,
        xedit_core::interface::types::ConflictAll,
        xedit_core::interface::types::ConflictThis,
    )> = response
        .node_conflicts
        .iter()
        .map(|node| (node.element, node.conflict_all.into(), node.conflict_this.into()))
        .collect();
    nav.install_filter(response.nodes.clone(), statuses);
    // Expand everything the filter kept and check that the visible records
    // are the CELLs and the WRLDs whose cells survived: the filter deleted
    // every other record from the tree (`CheckFilterNode` checks the leaves,
    // a record with surviving children stays).
    nav.expand_all();
    nav.prepare(None);
    let mut cells = 0;
    for row in 0..nav.row_count() {
        if let Some(element) = row_element(&nav, row)
            && let Some(signature) = signature_of(&element)
        {
            assert!(signature == "CELL" || signature == "WRLD", "row {row}: {signature}");
            if signature == "CELL" {
                cells += 1;
            }
        }
    }
    assert!(cells > 0, "Update.esm has cells");
    // `filter.remove` starts the tree again from the files.
    nav.remove_filter();
    nav.prepare(None);
    assert!(!nav.filter.applied);
    assert_eq!(nav.row_count(), nav.roots.len());
}
