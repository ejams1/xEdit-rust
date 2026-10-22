// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Headless checks of the menu model built from `xeMainForm.dfm`: every
//! popup of the form is modelled, every `mni*` binding the coverage ledger
//! tracks is an item of one of them, and the command each covered binding
//! runs is the ledger's.

use xedit_gui::menu_model::{self, MENUS, MenuItem, MenuKind};

/// Every popup menu `xeMainForm.dfm` declares.
const DFM_MENUS: [&str; 10] = [
    "pmuMain",
    "pmuNav",
    "pmuView",
    "pmuSpreadsheet",
    "pmuViewHeader",
    "pmuNavHeaderPopup",
    "pmuRefBy",
    "pmuNavAdd",
    "pmuMessages",
    "pmuBtnMenu",
];

fn all_items() -> Vec<&'static MenuItem> {
    let mut items = Vec::new();
    for tree in MENUS {
        menu_model::walk(tree.items, &mut |item| items.push(item));
    }
    items
}

#[test]
fn every_popup_menu_of_the_form_is_modelled() {
    let ids: Vec<&str> = MENUS.iter().map(|tree| tree.id).collect();
    assert_eq!(ids, DFM_MENUS);
}

#[test]
fn every_ledger_menu_binding_is_an_item_of_the_model() {
    // The ledger is the inventory of the form's GUI bindings; an item the
    // model leaves out is an upstream menu entry the GUI no longer shows.
    let ledger = include_str!("../../../coverage/ledger.toml");
    let mut missing = Vec::new();
    let mut checked = 0;
    for line in ledger.lines() {
        let Some(rest) = line.strip_prefix("[gui.\"xEdit/xeMainForm.dfm:") else {
            continue;
        };
        let Some(spec) = rest.strip_suffix("\"]") else {
            continue;
        };
        let mut parts = spec.split(':');
        let (Some(name), Some(event)) = (parts.next(), parts.next()) else {
            continue;
        };
        if event != "OnClick" || !name.starts_with("mni") {
            continue;
        }
        checked += 1;
        if !MENUS.iter().any(|tree| menu_model::item(tree, name).is_some()) {
            missing.push(name.to_owned());
        }
    }
    assert!(
        checked > 100,
        "the ledger should hold the main form's menu bindings, found {checked}"
    );
    assert!(missing.is_empty(), "menu bindings missing from the model: {missing:?}");
}

#[test]
fn the_commands_are_the_ledger_commands() {
    let ledger = include_str!("../../../coverage/ledger.toml");
    // Map each covered `mni*:OnClick` binding to its command.
    let mut expected: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut current: Option<String> = None;
    for line in ledger.lines() {
        if let Some(rest) = line.strip_prefix("[gui.\"xEdit/xeMainForm.dfm:") {
            let spec = rest.strip_suffix("\"]").unwrap_or(rest);
            let mut parts = spec.split(':');
            let (Some(name), Some(event)) = (parts.next(), parts.next()) else {
                continue;
            };
            current = (event == "OnClick" && name.starts_with("mni")).then(|| name.to_owned());
            continue;
        }
        if line.starts_with('[') {
            current = None;
            continue;
        }
        if let Some(name) = &current
            && let Some(command) = line.trim().strip_prefix("command = \"")
        {
            let command = command.strip_suffix('"').unwrap_or(command);
            expected.insert(name.clone(), command.to_owned());
        }
    }
    assert!(!expected.is_empty());
    for item in all_items() {
        if let Some(command) = expected.get(item.id) {
            assert_eq!(item.command, Some(command.as_str()), "{} should run {command}", item.id);
        }
    }
}

#[test]
fn every_visible_item_is_enabled() {
    // The DFM disables no item of the model: an item the form cannot act on
    // is drawn disabled by the form's own rule, not by the DFM.
    for item in all_items() {
        if item.kind == MenuKind::Separator {
            continue;
        }
        if item.visible {
            assert!(item.enabled, "{} is disabled in the DFM", item.id);
        }
    }
}

#[test]
fn separators_carry_no_handler() {
    for item in all_items() {
        if item.kind == MenuKind::Separator {
            assert_eq!(item.handler, None, "{} is a separator with a handler", item.id);
            assert_eq!(item.command, None);
            assert_eq!(item.caption, "-");
        } else {
            assert!(!item.caption.is_empty(), "{} has no caption", item.id);
        }
    }
}

#[test]
fn the_tree_keeps_the_upstream_shape() {
    // A few bindings the later steps build on, with their DFM captions and
    // handlers.
    let nav = &MENUS[1];
    let copy = menu_model::item(nav, "mniNavCopyAsOverride").unwrap();
    assert_eq!(copy.caption, "Copy as override into....");
    assert_eq!(copy.handler, Some("mniNavCopyIntoClick"));
    assert_eq!(copy.command, Some("records.copy"));
    let main = &MENUS[0];
    let save = menu_model::item(main, "mniMainSave").unwrap();
    assert_eq!(save.caption, "Save");
    assert_eq!(save.shortcut, Some("Ctrl+S"));
    assert_eq!(save.command, Some("files.save"));
    let messages = menu_model::item(MENUS[8], "mniMessagesAutoscroll").unwrap();
    assert!(messages.checked, "Autoscroll starts checked");
    // pmuNavAdd has no items: `mniNavAddClick` builds them at run time.
    assert!(MENUS[7].items.is_empty());
}
