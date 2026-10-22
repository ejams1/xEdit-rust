// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.dfm (pmuMain, pmuNav, pmuView,
// pmuSpreadsheet, pmuViewHeader, pmuNavHeaderPopup, pmuRefBy, pmuNavAdd,
// pmuMessages, pmuBtnMenu), coverage/ledger.toml (the command of each
// covered `gui.*` binding)

//! The menu tree of `xeMainForm`: the popup menus of the main form, with
//! the Caption, ShortCut, OnClick and check state of `xeMainForm.dfm` and
//! the session command each covered binding runs (the `gui.*` ledger
//! entries).
//!
//! The 4.1.5q GUI has no menu bar: `pmuMain` opens from the hamburger
//! button of the top bar, and the other popups are the context menus of
//! the navigation tree (`pmuNav`, `pmuNavHeaderPopup`), the view tree and
//! its header (`pmuView`, `pmuViewHeader`), the referenced-by list
//! (`pmuRefBy`), the messages memo (`pmuMessages`), the spreadsheets
//! (`pmuSpreadsheet`) and the button strip (`pmuBtnMenu`).
//!
//! Every item of the model is drawn: the form either runs the item's
//! command, or handles the item itself, so no upstream item is missing from
//! the form and none is drawn disabled.

use eframe::egui;

/// The command surface of one popup menu.
#[derive(Debug)]
pub struct MenuTree {
    /// The DFM component name (`pmuNav`).
    pub id: &'static str,
    pub items: &'static [MenuItem],
}

/// Shows the context menu of a widget at the point the click was made.
///
/// egui's own `Response::context_menu` anchors the menu at the pointer
/// position of the current frame. A pointer that left the window in the
/// same frame as the click (the window is told the cursor left it, and
/// egui then forgets where the pointer was) leaves the menu without a
/// place to be drawn, and it never opens. The click point survives the
/// leave, so the menu is anchored there instead.
pub fn context_menu(
    response: &egui::Response,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Option<egui::InnerResponse<()>> {
    let secondary = response.secondary_clicked();
    let primary = response.clicked();
    let click = response
        .interact_pointer_pos()
        .or_else(|| response.ctx.input(|input| input.pointer.interact_pos()));
    show_context_menu(response, secondary, primary, click, add_contents)
}

/// Shows the context menu of a container at the point the click was made.
///
/// The container has no sense of its own, so the click is read from the
/// pointer state: the widget under the pointer is the one that was
/// clicked. See [`context_menu`] for why the anchor is the click point
/// and not the pointer position of the frame.
pub fn container_context_menu(
    response: &egui::Response,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Option<egui::InnerResponse<()>> {
    let secondary = response.container_secondary_clicked();
    let primary = response.container_clicked();
    let click = response
        .ctx
        .input(|input| input.pointer.interact_pos())
        .filter(|_| secondary || primary);
    show_context_menu(response, secondary, primary, click, add_contents)
}

/// The point a context menu was opened at.
///
/// The point is only known in the frame of the click; the frames after it
/// have no pointer to read it from, and the menu would then be drawn at
/// the widget's own corner. The point is therefore remembered for as long
/// as the menu is open.
fn anchor_id(response: &egui::Response) -> egui::Id {
    egui::Popup::default_response_id(response).with("anchor")
}

/// Shows a context menu, opened at `click`, for as long as it is open.
fn show_context_menu(
    response: &egui::Response,
    secondary: bool,
    primary: bool,
    click: Option<egui::Pos2>,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> Option<egui::InnerResponse<()>> {
    let anchor = anchor_id(response);
    if secondary && let Some(click) = click {
        response.ctx.data_mut(|data| data.insert_temp(anchor, click));
    }
    let position = response.ctx.data(|data| data.get_temp::<egui::Pos2>(anchor));
    let mut popup = egui::Popup::menu(response)
        .kind(egui::PopupKind::Menu)
        .open_memory(if secondary {
            Some(egui::SetOpenCommand::Bool(true))
        } else if primary {
            // A click on the widget closes the menu again.
            Some(egui::SetOpenCommand::Bool(false))
        } else {
            None
        });
    if let Some(position) = position {
        popup = popup.at_position(position);
    }
    let shown = popup.show(add_contents);
    if std::env::var_os("XEDIT_GUI_DEBUG_GRID").is_some() && (secondary || primary || shown.is_some()) {
        crate::log_screen(
            "popup",
            &[format!(
                "id={:?} secondary={secondary} primary={primary} click={click:?} position={position:?} shown={}",
                response.id,
                shown.is_some()
            )],
        );
    }
    shown
}

/// The state a form gives one item when it draws it: the check box of a
/// toggle (`AutoCheck`) and whether the form's own rules allow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItemState {
    pub enabled: bool,
    pub checked: bool,
}

impl MenuItemState {
    /// The DFM state (`item.checked`, enabled unless the DFM disables it).
    pub fn of(item: &MenuItem) -> Self {
        Self {
            enabled: item.enabled,
            checked: item.checked,
        }
    }
}

/// Draws a popup menu from the model and returns the clicked item.
///
/// `state` is the form's own answer for an item (its check box and whether
/// a selection allows it).
pub fn draw_menu(
    ui: &mut egui::Ui,
    tree: &'static MenuTree,
    state: &dyn Fn(&MenuItem) -> MenuItemState,
) -> Option<&'static MenuItem> {
    draw_items(ui, tree.items, state)
}

fn draw_items(
    ui: &mut egui::Ui,
    items: &'static [MenuItem],
    state: &dyn Fn(&MenuItem) -> MenuItemState,
) -> Option<&'static MenuItem> {
    let mut chosen = None;
    for item in items {
        if !item.visible {
            continue;
        }
        match item.kind {
            MenuKind::Separator => {
                ui.separator();
            }
            MenuKind::Submenu => {
                ui.menu_button(item.caption, |ui| {
                    if let Some(item) = draw_items(ui, item.children, state) {
                        chosen = Some(item);
                    }
                });
            }
            MenuKind::RuntimeSubmenu => {
                // The items of such a menu belong to the session; the form
                // that owns the session draws them (`TfrmMain`).
                ui.add_enabled(false, egui::Button::new(item.caption))
                    .on_disabled_hover_text("The items of this menu are built at run time");
            }
            MenuKind::Action => {
                let own = state(item);
                let mut button = egui::Button::new(item.caption).selected(own.checked);
                if let Some(shortcut) = item.shortcut {
                    button = button.shortcut_text(shortcut);
                }
                let response = ui.add_enabled(own.enabled, button);
                if response.clicked() {
                    chosen = Some(item);
                    ui.close();
                }
            }
        }
    }
    chosen
}

/// The kind of an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    /// A command item.
    Action,
    /// A `-` separator.
    Separator,
    /// A submenu; its items are in `children`.
    Submenu,
    /// A submenu whose items are built at run time: upstream fills such a
    /// menu in a popup handler (`pmuMainPopup` fills
    /// `mniMainLocalizationLanguage` with the languages of the session),
    /// so the items are not in the model but in a [`RuntimeMenu`].
    RuntimeSubmenu,
}

/// One item of a popup menu.
#[derive(Debug)]
pub struct MenuItem {
    /// The DFM object name (`mniNavCopyAsOverride`).
    pub id: &'static str,
    /// The Caption without the Delphi `&` accelerator marker.
    pub caption: &'static str,
    /// The ShortCut as the DFM stores it, rendered (`Ctrl+S`).
    pub shortcut: Option<&'static str>,
    /// The `OnClick` handler (`mniNavCopyIntoClick`).
    pub handler: Option<&'static str>,
    /// The session command the covered binding runs; `None` for a
    /// presentation-only or excluded binding.
    pub command: Option<&'static str>,
    pub kind: MenuKind,
    /// `Checked = True` of the DFM.
    pub checked: bool,
    /// `RadioItem = True` of the DFM.
    pub radio: bool,
    /// `Enabled = False` of the DFM.
    pub enabled: bool,
    /// `Visible = False` of the DFM.
    pub visible: bool,
    /// The items of a submenu.
    pub children: &'static [MenuItem],
}

/// A submenu whose items the form builds at run time.
///
/// Upstream fills such a menu when its popup opens: `pmuMainPopup` clears
/// `mniMainLocalizationLanguage` and adds one radio item per language of
/// the session, the current one checked. The list belongs to the session,
/// not to the model, so the model names the item and the form hands the
/// items to the drawing code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeMenu {
    /// The item the items belong to (`mniMainLocalizationLanguage`).
    pub id: &'static str,
    /// The items, in the order upstream lists them.
    pub items: Vec<RuntimeItem>,
}

/// One item of a run-time submenu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeItem {
    /// The caption of the item (the name of a language).
    pub caption: String,
    /// The session command the item runs (`localization.language`).
    pub command: &'static str,
    /// `Checked = True` of the radio item: the language of the session.
    pub checked: bool,
    /// The parameter the command takes (the language to switch to).
    pub param: String,
}

pub const PMU_MAIN: MenuTree = MenuTree {
    id: "pmuMain",
    items: &[
        MenuItem {
            id: "mniMainLocalization",
            caption: "Localization",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Submenu,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[
                MenuItem {
                    id: "mniMainLocalizationLanguage",
                    caption: "Language",
                    shortcut: None,
                    // Upstream binds the handler to the items it adds at
                    // run time, not to the item of the DFM.
                    handler: Some("mniMainLocalizationLanguageClick"),
                    // The command of the item is the command its run-time
                    // items run, with the language as its parameter.
                    command: Some("localization.language"),
                    // The items are the languages of the session
                    // (`pmuMainPopup`), so the menu is built at run time.
                    kind: MenuKind::RuntimeSubmenu,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniMainLocalizationEditor",
                    caption: "Editor",
                    shortcut: None,
                    handler: Some("mniMainLocalizationEditorClick"),
                    command: Some("localization.files"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
            ],
        },
        MenuItem {
            id: "mniMainPluggyLink",
            caption: "Pluggy Link",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Submenu,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[
                MenuItem {
                    id: "mniMainPluggyLinkDisabled",
                    caption: "Disabled",
                    shortcut: None,
                    handler: Some("mniMainPluggyLinkClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: true,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniMainPluggyLinkReference",
                    caption: "Reference",
                    shortcut: None,
                    handler: Some("mniMainPluggyLinkClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniMainPluggyLinkBaseObject",
                    caption: "Base Object",
                    shortcut: None,
                    handler: Some("mniMainPluggyLinkClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniMainPluggyLinkInventory",
                    caption: "Inventory",
                    shortcut: None,
                    handler: Some("mniMainPluggyLinkClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniMainPluggyLinkEnchantment",
                    caption: "Enchantment",
                    shortcut: None,
                    handler: Some("mniMainPluggyLinkClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniMainPluggyLinkSpell",
                    caption: "Spell",
                    shortcut: None,
                    handler: Some("mniMainPluggyLinkClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
            ],
        },
        MenuItem {
            id: "N30",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniMainSave",
            caption: "Save",
            shortcut: Some("Ctrl+S"),
            handler: Some("mniMainSaveClick"),
            command: Some("files.save"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N31",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniMainOptions",
            caption: "Options",
            shortcut: Some("Ctrl+O"),
            handler: Some("mniNavOptionsClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
    ],
};

pub const PMU_NAV: MenuTree = MenuTree {
    id: "pmuNav",
    items: &[
        MenuItem {
            id: "mniNavCompareTo",
            caption: "Compare to...",
            shortcut: None,
            handler: Some("mniNavCompareToClick"),
            command: Some("files.compare_to"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCreateDeltaPatch",
            caption: "Create delta patch using...",
            shortcut: None,
            handler: Some("mniNavCreateDeltaPatchClick"),
            command: Some("files.create_delta_patch"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCompareSelected",
            caption: "Compare Selected",
            shortcut: None,
            handler: Some("mniNavCompareSelectedClick"),
            command: Some("records.compare_selected"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N3",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavFilterRemove",
            caption: "Remove Filter",
            shortcut: None,
            handler: Some("mniNavFilterRemoveClick"),
            command: Some("filter.remove"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavFilterApply",
            caption: "Apply Filter",
            shortcut: None,
            handler: Some("mniNavFilterApplyClick"),
            command: Some("filter.apply"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavFilterForCleaning",
            caption: "Apply Filter for Cleaning",
            shortcut: None,
            handler: Some("mniNavFilterForCleaningClick"),
            command: Some("files.clean"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavFilterForCleaningObsolete",
            caption: "Apply Filter for Cleaning",
            shortcut: None,
            handler: Some("mniNavCleaningObsoleteClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavFilterConflicts",
            caption: "Apply Filter to show Conflicts",
            shortcut: None,
            handler: Some("mniNavFilterConflictsClick"),
            command: Some("filter.apply"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N25",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavFilterApplySelected",
            caption: "Apply Filter (selected files only)",
            shortcut: None,
            handler: Some("mniNavFilterApplyClick"),
            command: Some("filter.apply"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavFilterForCleaningSelected",
            caption: "Apply Filter for Cleaning (selected files only)",
            shortcut: None,
            handler: Some("mniNavFilterForCleaningClick"),
            command: Some("files.clean"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavFilterForCleaningSelectedObsolete",
            caption: "Apply Filter for Cleaning (selected files only)",
            shortcut: None,
            handler: Some("mniNavCleaningObsoleteClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavFilterConflictsSelected",
            caption: "Apply Filter to show Conflicts (selected files only)",
            shortcut: None,
            handler: Some("mniNavFilterConflictsClick"),
            command: Some("filter.apply"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N1",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCheckForErrors",
            caption: "Check for Errors",
            shortcut: None,
            handler: Some("mniNavCheckForErrorsClick"),
            command: Some("files.check"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCheckForCircularLeveledLists",
            caption: "Check for Circular Leveled Lists",
            shortcut: None,
            handler: Some("mniNavCheckForCircularLeveledListsClick"),
            command: Some("leveled_lists.check_circular"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N2",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavChangeFormID",
            caption: "Change FormID",
            shortcut: None,
            handler: Some("mniNavChangeFormIDClick"),
            command: Some("formids.change"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavChangeReferencingRecords",
            caption: "Change Referencing Records",
            shortcut: None,
            handler: Some("mniNavChangeReferencingRecordsClick"),
            command: Some("references.change"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavRenumberFormIDsFrom",
            caption: "Renumber FormIDs from...",
            shortcut: None,
            handler: Some("mniNavRenumberFormIDsFromClick"),
            command: Some("formids.renumber"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCompactFormIDs",
            caption: "Compact FormIDs for ESL",
            shortcut: None,
            handler: Some("mniNavRenumberFormIDsFromClick"),
            command: Some("formids.renumber"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavRenumberFormIDsInject",
            caption: "Inject Forms into master...",
            shortcut: None,
            handler: Some("mniNavRenumberFormIDsFromClick"),
            command: Some("formids.renumber"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N19",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavApplyScript",
            caption: "Apply Script...",
            shortcut: None,
            handler: Some("mniNavApplyScriptClick"),
            command: Some("script.run"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N18",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavUndeleteAndDisableReferences",
            caption: "Undelete and Disable References",
            shortcut: None,
            handler: Some("mniNavUndeleteAndDisableReferencesClick"),
            command: Some("files.clean"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavUndeleteAndDisableReferencesObsolete",
            caption: "Undelete and Disable References",
            shortcut: None,
            handler: Some("mniNavCleaningObsoleteClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavRemoveIdenticalToMaster",
            caption: "Remove \"Identical to Master\" records",
            shortcut: None,
            handler: Some("mniNavRemoveIdenticalToMasterClick"),
            command: Some("files.clean"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavRemoveIdenticalToMasterObsolete",
            caption: "Remove \"Identical to Master\" records",
            shortcut: None,
            handler: Some("mniNavCleaningObsoleteClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavLOManagersDirtyInfo",
            caption: "BOSS/LOOT Cleaning Report",
            shortcut: None,
            handler: Some("mniNavLOManagersDirtyInfoClick"),
            command: Some("files.dirty_info"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N17",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavSetVWDAuto",
            caption: "Set VWD for all REFR with VWD Mesh in this file",
            shortcut: None,
            handler: Some("mniNavSetVWDAutoClick"),
            command: Some("references.set_vwd_auto"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavSetVWDAutoInto",
            caption: "Set VWD for all REFR with VWD Mesh as Override into....",
            shortcut: None,
            handler: Some("mniNavSetVWDAutoIntoClick"),
            command: Some("references.copy_vwd_auto"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N15",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCellChildTemp",
            caption: "Temporary",
            shortcut: None,
            handler: Some("mniNavCellChild"),
            command: Some("records.set_persistent"),
            kind: MenuKind::Action,
            checked: false,
            radio: true,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCellChildPers",
            caption: "Persistent",
            shortcut: None,
            handler: Some("mniNavCellChild"),
            command: Some("records.set_persistent"),
            kind: MenuKind::Action,
            checked: false,
            radio: true,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCellChildNotVWD",
            caption: "not Visible When Distant",
            shortcut: None,
            handler: Some("mniNavCellChild"),
            command: Some("records.set_visible_when_distant"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCellChildVWD",
            caption: "Visible When Distant",
            shortcut: None,
            handler: Some("mniNavCellChild"),
            command: Some("records.set_visible_when_distant"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N32",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniCreateNewFile",
            caption: "Create New File...",
            shortcut: None,
            handler: Some("mniCreateNewFileClick"),
            command: Some("files.new"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N5",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavAdd",
            caption: "Add",
            shortcut: None,
            handler: Some("mniNavAddClick"),
            command: Some("elements.add"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavRemove",
            caption: "Remove",
            shortcut: None,
            handler: Some("mniNavRemoveClick"),
            command: Some("records.delete"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavMarkModified",
            caption: "Mark Modified",
            shortcut: None,
            handler: Some("mniNavMarkModifiedClick"),
            command: Some("elements.mark_modified"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N6",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavAddMasters",
            caption: "Add Masters...",
            shortcut: None,
            handler: Some("mniNavAddMastersClick"),
            command: Some("masters.add"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavSortMasters",
            caption: "Sort Masters (to match current load order)",
            shortcut: None,
            handler: Some("mniNavSortMastersClick"),
            command: Some("masters.sort"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCleanMasters",
            caption: "Clean Masters (= Remove all unused Masters)",
            shortcut: None,
            handler: Some("mniNavCleanMastersClick"),
            command: Some("masters.clean"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N23",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCreateModGroup",
            caption: "Create ModGroup...",
            shortcut: None,
            handler: Some("mniNavCreateModGroupClick"),
            command: Some("modgroups.create"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavEditModGroup",
            caption: "Edit ModGroup...",
            shortcut: None,
            handler: Some("mniNavEditModGroupClick"),
            command: Some("modgroups.edit"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavDeleteModGroups",
            caption: "Delete ModGroups...",
            shortcut: None,
            handler: Some("mniNavDeleteModGroupsClick"),
            command: Some("modgroups.delete"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavUpdateCRCModGroups",
            caption: "Update CRC in ModGroups...",
            shortcut: None,
            handler: Some("mniNavUpdateCRCModGroupsClick"),
            command: Some("modgroups.update_crcs"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N4",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCopyAsOverride",
            caption: "Copy as override into....",
            shortcut: None,
            handler: Some("mniNavCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCopyAsOverrideWithOverwrite",
            caption: "Copy as override (with overwriting) into....",
            shortcut: None,
            handler: Some("mniNavCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavDeepCopyAsOverride",
            caption: "Deep copy as override into....",
            shortcut: None,
            handler: Some("mniNavCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavDeepCopyAsOverrideWithOverwriting",
            caption: "Deep copy as override (with overwriting) into....",
            shortcut: None,
            handler: Some("mniNavCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCopyAsNewRecord",
            caption: "Copy as new record into...",
            shortcut: None,
            handler: Some("mniNavCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCopyAsSpawnRateOverride",
            caption: "Copy as override (spawn rate plugin) into...",
            shortcut: None,
            handler: Some("mniNavCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCopyAsWrapper",
            caption: "Copy as wrapper into...",
            shortcut: None,
            handler: Some("mniNavCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCleanupInjected",
            caption: "Cleanup references to injected records",
            shortcut: None,
            handler: Some("mniNavCleanupInjectedClick"),
            command: Some("records.cleanup_injected"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavCopyIdle",
            caption: "Copy Idle Animations into...",
            shortcut: None,
            handler: Some("mniNavCopyIdleClick"),
            command: Some("records.copy_idles"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N10",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavHidden",
            caption: "Hidden",
            shortcut: None,
            handler: Some("mniNavHiddenClick"),
            command: Some("records.set_hidden"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N16",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavTest",
            caption: "Test",
            shortcut: None,
            handler: Some("mniNavTestClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniNavBanditFix",
            caption: "Bandit Fix",
            shortcut: None,
            handler: Some("mniNavBanditFixClick"),
            command: Some("records.bandit_fix"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: false,
            children: &[],
        },
        MenuItem {
            id: "mniNavOther",
            caption: "Other",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Submenu,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[
                MenuItem {
                    id: "mniNavCreateMergedPatch",
                    caption: "Create Merged Patch",
                    shortcut: None,
                    handler: Some("mniNavCreateMergedPatchClick"),
                    command: Some("patch.merged"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavCreateSEQFile",
                    caption: "Create SEQ File",
                    shortcut: None,
                    handler: Some("mniNavCreateSEQFileClick"),
                    command: Some("tool.run"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavGenerateLOD",
                    caption: "Generate LOD",
                    shortcut: None,
                    handler: Some("mniNavGenerateLODClick"),
                    command: Some("lodgen.generate"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavBuildRef",
                    caption: "Build Reference Info",
                    shortcut: None,
                    handler: Some("mniNavBuildRefClick"),
                    command: Some("refs.build"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavBuildReachable",
                    caption: "Build Reachable Info",
                    shortcut: None,
                    handler: Some("mniNavBuildReachableClick"),
                    command: Some("refs.build_reachable"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavBatchChangeReferencingRecords",
                    caption: "Batch Change Referencing Records",
                    shortcut: None,
                    handler: Some("mniNavBatchChangeReferencingRecordsClick"),
                    command: Some("references.change_batch"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavRaceLVLIs",
                    caption: "Fixup Race-specific LVLIs",
                    shortcut: None,
                    handler: Some("mniNavRaceLVLIsClick"),
                    command: Some("records.race_leveled_lists"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: false,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavLocalization",
                    caption: "Localization",
                    shortcut: None,
                    handler: None,
                    command: None,
                    kind: MenuKind::Submenu,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[MenuItem {
                        id: "mniNavLocalizationSwitch",
                        caption: "Localize",
                        shortcut: None,
                        handler: Some("mniNavLocalizationSwitchClick"),
                        command: Some("localization.localize"),
                        kind: MenuKind::Action,
                        checked: false,
                        radio: false,
                        enabled: true,
                        visible: true,
                        children: &[],
                    }],
                },
                MenuItem {
                    id: "mniNavLogAnalyzer",
                    caption: "Log Analyzer",
                    shortcut: None,
                    handler: None,
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniMarkallfileswithoutONAMasmodified",
                    caption: "Mark all files without ONAM as modified",
                    shortcut: None,
                    handler: Some("mniMarkallfileswithoutONAMasmodifiedClick"),
                    command: Some("files.mark_modified_headers"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "N13",
                    caption: "-",
                    shortcut: None,
                    handler: None,
                    command: None,
                    kind: MenuKind::Separator,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavOptions",
                    caption: "Options",
                    shortcut: None,
                    handler: Some("mniNavOptionsClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavOtherCodeSiteLogging",
                    caption: "CodeSite logging",
                    shortcut: None,
                    handler: Some("mniNavOtherCodeSiteLoggingClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
            ],
        },
    ],
};

pub const PMU_VIEW: MenuTree = MenuTree {
    id: "pmuView",
    items: &[
        MenuItem {
            id: "mniViewEdit",
            caption: "Edit",
            shortcut: None,
            handler: Some("mniViewEditClick"),
            command: Some("elements.set"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewAdd",
            caption: "Add",
            shortcut: None,
            handler: Some("mniViewAddClick"),
            command: Some("elements.add"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N26",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewRemove",
            caption: "Remove",
            shortcut: None,
            handler: Some("mniViewRemoveClick"),
            command: Some("elements.remove"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewClear",
            caption: "Clear",
            shortcut: None,
            handler: Some("mniViewClearClick"),
            command: Some("elements.clear"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewRemoveFromSelected",
            caption: "Remove from selected records",
            shortcut: None,
            handler: Some("mniViewRemoveFromSelectedClick"),
            command: Some("elements.remove_from_records"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N27",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewNextMember",
            caption: "Next Member",
            shortcut: None,
            handler: Some("mniViewNextMemberClick"),
            command: Some("elements.change_member"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewPreviousMember",
            caption: "Previous Member",
            shortcut: None,
            handler: Some("mniViewPreviousMemberClick"),
            command: Some("elements.change_member"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N28",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewSetToDefault",
            caption: "Reset structure",
            shortcut: None,
            handler: Some("mniViewSetToDefaultClick"),
            command: Some("elements.set"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N29",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewCopyToSelectedRecords",
            caption: "Copy to selected records",
            shortcut: None,
            handler: Some("mniViewCopyToSelectedRecordsClick"),
            command: Some("elements.copy_to_records"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewCopyMultipleToSelectedRecords",
            caption: "Copy multiple to selected records",
            shortcut: None,
            handler: Some("mniViewCopyMultipleToSelectedRecordsClick"),
            command: Some("elements.copy_to_records"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N12",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewMoveUp",
            caption: "Move &up",
            shortcut: None,
            handler: Some("mniViewMoveUpClick"),
            command: Some("elements.move"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewMoveDown",
            caption: "Move &down",
            shortcut: None,
            handler: Some("mniViewMoveDownClick"),
            command: Some("elements.move"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N8",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewSort",
            caption: "Sort by this row",
            shortcut: None,
            handler: Some("mniViewSortClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewCompareReferencedRow",
            caption: "Compare referenced records in this row",
            shortcut: None,
            handler: Some("mniViewCompareReferencedRowClick"),
            command: Some("records.compare_selected"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N9",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewClipboard",
            caption: "Clipboard",
            shortcut: None,
            handler: Some("mniViewClipboardClick"),
            command: None,
            kind: MenuKind::Submenu,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[
                MenuItem {
                    id: "mniCopyPathToClipboard",
                    caption: "Copy path",
                    shortcut: None,
                    handler: Some("mniCopyPathToClipboardClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniCopyFullPathToClipboard",
                    caption: "Copy full path",
                    shortcut: None,
                    handler: Some("mniCopyFullPathToClipboardClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniCopyIndexedPathToClipBoard",
                    caption: "Copy indexed path",
                    shortcut: None,
                    handler: Some("mniCopyIndexedPathToClipboardClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniCopyPathNameToClipboard",
                    caption: "Copy full path (short names)",
                    shortcut: None,
                    handler: Some("mniCopyPathNameToClipboardClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniClipboardSeparator",
                    caption: "-",
                    shortcut: None,
                    handler: None,
                    command: None,
                    kind: MenuKind::Separator,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniCopySignatureToClipboard",
                    caption: "Copy signature",
                    shortcut: None,
                    handler: Some("mniCopySignatureToClipboardClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniCopyNameToClipboard",
                    caption: "Copy name",
                    shortcut: None,
                    handler: Some("mniCopyNameToClipboardClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniCopyDisplayNameToClipboard",
                    caption: "Copy display name",
                    shortcut: None,
                    handler: Some("mniCopyDisplayNameToClipboardClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniCopyShortNameToClipboard",
                    caption: "Copy short name",
                    shortcut: None,
                    handler: Some("mniCopyShortNameToClipboardClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
            ],
        },
        MenuItem {
            id: "mniViewClipboardSeparator",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewHideNoConflict",
            caption: "Hide no conflict and empty rows",
            shortcut: None,
            handler: Some("mniViewHideNoConflictClick"),
            command: Some("records.compare"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewStick",
            caption: "Stick to",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Submenu,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[
                MenuItem {
                    id: "mniViewStickAuto",
                    caption: "Auto Top Row",
                    shortcut: None,
                    handler: Some("mniViewStickAutoClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniViewStickSelected",
                    caption: "Selected Row",
                    shortcut: None,
                    handler: Some("mniViewStickSelectedClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
            ],
        },
        MenuItem {
            id: "ColumnWidths1",
            caption: "Column Widths",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Submenu,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[
                MenuItem {
                    id: "mniViewColumnWidthStandard",
                    caption: "Standard",
                    shortcut: None,
                    handler: Some("mniViewColumnWidthClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniViewColumnWidthFitAll",
                    caption: "Fit All",
                    shortcut: None,
                    handler: Some("mniViewColumnWidthClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniViewColumnWidthFitText",
                    caption: "Fit Text",
                    shortcut: None,
                    handler: Some("mniViewColumnWidthClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniViewColumnWidthFitSmart",
                    caption: "Fit Smart",
                    shortcut: None,
                    handler: Some("mniViewColumnWidthClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
            ],
        },
        MenuItem {
            id: "mniModGroups",
            caption: "ModGroups",
            shortcut: None,
            handler: Some("mniModGroupsClick"),
            command: None,
            kind: MenuKind::Submenu,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[
                MenuItem {
                    id: "mniModGroupsEnabled",
                    caption: "Enabled",
                    shortcut: None,
                    handler: Some("mniModGroupsAbleClick"),
                    command: Some("conflicts.list"),
                    kind: MenuKind::Action,
                    checked: true,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniModGroupsDisabled",
                    caption: "Disabled",
                    shortcut: None,
                    handler: Some("mniModGroupsAbleClick"),
                    command: Some("conflicts.list"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "N22",
                    caption: "-",
                    shortcut: None,
                    handler: None,
                    command: None,
                    kind: MenuKind::Separator,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniViewModGroupsReload",
                    caption: "Reload ModGroups",
                    shortcut: None,
                    handler: Some("mniViewModGroupsReloadClick"),
                    command: Some("modgroups.select"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: false,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
            ],
        },
        MenuItem {
            id: "mniMasterAndLeafs",
            caption: "Only Master and Leafs",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Submenu,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[
                MenuItem {
                    id: "mniMasterAndLeafsEnabled",
                    caption: "Enabled",
                    shortcut: None,
                    handler: Some("mniMasterAndLeafsClick"),
                    command: Some("records.compare"),
                    kind: MenuKind::Action,
                    checked: true,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniMasterAndLeafsDisabled",
                    caption: "Disabled",
                    shortcut: None,
                    handler: Some("mniMasterAndLeafsClick"),
                    command: Some("records.compare"),
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
            ],
        },
    ],
};

pub const PMU_SPREADSHEET: MenuTree = MenuTree {
    id: "pmuSpreadsheet",
    items: &[
        MenuItem {
            id: "mniSpreadsheetCompareSelected",
            caption: "Compare Selected",
            shortcut: None,
            handler: Some("mniSpreadsheetCompareSelectedClick"),
            command: Some("records.compare_selected"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N7",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniSpreadsheetRebuild",
            caption: "Rebuild",
            shortcut: None,
            handler: Some("mniSpreadsheetRebuildClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
    ],
};

pub const PMU_VIEW_HEADER: MenuTree = MenuTree {
    id: "pmuViewHeader",
    items: &[
        MenuItem {
            id: "mniViewHeaderCopyAsOverride",
            caption: "Copy as override into....",
            shortcut: None,
            handler: Some("mniViewHeaderCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewHeaderCopyAsOverrideWithOverwriting",
            caption: "Copy as override (with overwriting) into....",
            shortcut: None,
            handler: Some("mniViewHeaderCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewHeaderDeepCopyAsOverride",
            caption: "Deep copy as override into....",
            shortcut: None,
            handler: Some("mniViewHeaderCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewHeaderDeepCopyAsOverrideWithOverwriting",
            caption: "Deep copy as override (with overwriting) into....",
            shortcut: None,
            handler: Some("mniViewHeaderCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewHeaderCopyAsNewRecord",
            caption: "Copy as new record into...",
            shortcut: None,
            handler: Some("mniViewHeaderCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewHeaderCopyAsWrapper",
            caption: "Copy as wrapper into...",
            shortcut: None,
            handler: Some("mniViewHeaderCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewHeaderRemove",
            caption: "Remove",
            shortcut: None,
            handler: Some("mniViewHeaderRemoveClick"),
            command: Some("records.delete"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewHeaderJumpTo",
            caption: "Jump to",
            shortcut: None,
            handler: Some("mniViewHeaderJumpToClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N24",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewCreateModGroup",
            caption: "Create ModGroup...",
            shortcut: None,
            handler: Some("mniNavCreateModGroupClick"),
            command: Some("modgroups.create"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N11",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewHeaderHidden",
            caption: "Hide",
            shortcut: None,
            handler: Some("mniViewHeaderHiddenClick"),
            command: Some("records.set_hidden"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniViewHeaderUnhideAll",
            caption: "Unhide all...",
            shortcut: None,
            handler: Some("mniViewHeaderUnhideAllClick"),
            command: Some("records.set_hidden"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
    ],
};

pub const PMU_NAV_HEADER_POPUP: MenuTree = MenuTree {
    id: "pmuNavHeaderPopup",
    items: &[
        MenuItem {
            id: "mniNavHeaderFiles",
            caption: "Files",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Submenu,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[
                MenuItem {
                    id: "mniNavHeaderFilesDefault",
                    caption: "as selected",
                    shortcut: None,
                    handler: Some("mniNavHeaderFilesClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: true,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavHeaderFilesLoadOrder",
                    caption: "always by load order",
                    shortcut: None,
                    handler: Some("mniNavHeaderFilesClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavHeaderFilesFileName",
                    caption: "always by file name",
                    shortcut: None,
                    handler: Some("mniNavHeaderFilesClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
            ],
        },
        MenuItem {
            id: "mniNavHeaderINFO",
            caption: "Dialog Topics",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Submenu,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[
                MenuItem {
                    id: "mniNavHeaderINFObyPreviousINFO",
                    caption: "by Previous INFO",
                    shortcut: None,
                    handler: Some("mniNavHeaderINFOClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: true,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
                MenuItem {
                    id: "mniNavHeaderINFObyFormID",
                    caption: "by FormID",
                    shortcut: None,
                    handler: Some("mniNavHeaderINFOClick"),
                    command: None,
                    kind: MenuKind::Action,
                    checked: false,
                    radio: true,
                    enabled: true,
                    visible: true,
                    children: &[],
                },
            ],
        },
    ],
};

pub const PMU_REF_BY: MenuTree = MenuTree {
    id: "pmuRefBy",
    items: &[
        MenuItem {
            id: "mniRefByCompareSelected",
            caption: "Compare Selected",
            shortcut: None,
            handler: Some("mniRefByCompareSelectedClick"),
            command: Some("records.compare_selected"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N33",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByApplyScript",
            caption: "Apply Script...",
            shortcut: None,
            handler: Some("mniNavApplyScriptClick"),
            command: Some("script.run"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N14",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByCopyOverrideInto",
            caption: "Copy as override into....",
            shortcut: None,
            handler: Some("mniRefByCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByCopyOverrideIntoWithOverwriting",
            caption: "Copy as override (with overwriting) into....",
            shortcut: None,
            handler: Some("mniRefByCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByDeepCopyOverrideInto",
            caption: "Deep copy as override into....",
            shortcut: None,
            handler: Some("mniRefByCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByDeepCopyOverrideIntoWithOverwriting",
            caption: "Deep copy as override (with overwriting) into....",
            shortcut: None,
            handler: Some("mniRefByCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByCopyAsNewInto",
            caption: "Copy as new record into...",
            shortcut: None,
            handler: Some("mniRefByCopyIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByCopyDisabledOverrideInto",
            caption: "Copy as disabled override into....",
            shortcut: None,
            handler: Some("mniRefByCopyDisabledOverrideIntoClick"),
            command: Some("records.copy"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N20",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByRemove",
            caption: "Remove",
            shortcut: None,
            handler: Some("mniRefByRemoveClick"),
            command: Some("records.delete"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByMarkModified",
            caption: "Mark Modified",
            shortcut: None,
            handler: Some("mniRefByMarkModifiedClick"),
            command: Some("elements.mark_modified"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByVWD",
            caption: "Visible When Distant",
            shortcut: None,
            handler: Some("mniRefByVWDClick"),
            command: Some("records.set_visible_when_distant"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniRefByNotVWD",
            caption: "not Visible When Distant",
            shortcut: None,
            handler: Some("mniRefByVWDClick"),
            command: Some("records.set_visible_when_distant"),
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
    ],
};

pub const PMU_NAV_ADD: MenuTree = MenuTree {
    id: "pmuNavAdd",
    items: &[],
};

pub const PMU_MESSAGES: MenuTree = MenuTree {
    id: "pmuMessages",
    items: &[
        MenuItem {
            id: "mniMessagesClear",
            caption: "Clear",
            shortcut: None,
            handler: Some("mniMessagesClearClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniMessagesSaveSelected",
            caption: "Save selected text",
            shortcut: None,
            handler: Some("mniMessagesSaveSelectedClick"),
            command: None,
            kind: MenuKind::Action,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "N21",
            caption: "-",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Separator,
            checked: false,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
        MenuItem {
            id: "mniMessagesAutoscroll",
            caption: "Autoscroll to the last message",
            shortcut: None,
            handler: None,
            command: None,
            kind: MenuKind::Action,
            checked: true,
            radio: false,
            enabled: true,
            visible: true,
            children: &[],
        },
    ],
};

pub const PMU_BTN_MENU: MenuTree = MenuTree {
    id: "pmuBtnMenu",
    items: &[MenuItem {
        id: "mniBtnShrinkButtons",
        caption: "Shrink Buttons",
        shortcut: None,
        handler: Some("mniBtnShrinkButtonsClick"),
        command: None,
        kind: MenuKind::Action,
        checked: false,
        radio: false,
        enabled: true,
        visible: true,
        children: &[],
    }],
};

/// Every popup menu of the main form, in DFM order.
pub const MENUS: &[&MenuTree] = &[
    &PMU_MAIN,
    &PMU_NAV,
    &PMU_VIEW,
    &PMU_SPREADSHEET,
    &PMU_VIEW_HEADER,
    &PMU_NAV_HEADER_POPUP,
    &PMU_REF_BY,
    &PMU_NAV_ADD,
    &PMU_MESSAGES,
    &PMU_BTN_MENU,
];

/// Finds an item by its DFM name, depth first.
pub fn item(tree: &'static MenuTree, id: &str) -> Option<&'static MenuItem> {
    fn find(items: &'static [MenuItem], id: &str) -> Option<&'static MenuItem> {
        for item in items {
            if item.id == id {
                return Some(item);
            }
            if let Some(found) = find(item.children, id) {
                return Some(found);
            }
        }
        None
    }
    find(tree.items, id)
}

/// Every item of a tree in DFM order, separators included.
pub fn walk(items: &'static [MenuItem], visit: &mut impl FnMut(&'static MenuItem)) {
    for item in items {
        visit(item);
        walk(item.children, visit);
    }
}

/// The manual cleaning items of the navigation popup come in a current and
/// an obsolete variant with the same caption; `xeMainForm.pas:15478` shows
/// one of each pair. `wbManualCleaningAllow` picks the current one,
/// otherwise the obsolete one is shown (`wbManualCleaningHide` hides it).
/// The items of the "check for errors" group follow
/// `mniNavCheckForErrors.Visible`.
///
/// `None` for an item that is not one of the eight.
pub fn manual_cleaning_visible(id: &str, allow: bool, hide: bool, check_for_errors: bool) -> Option<bool> {
    let needs_check = match id {
        "mniNavFilterForCleaning" | "mniNavFilterForCleaningObsolete" => false,
        "mniNavFilterForCleaningSelected" | "mniNavFilterForCleaningSelectedObsolete" => false,
        "mniNavRemoveIdenticalToMaster" | "mniNavRemoveIdenticalToMasterObsolete" => true,
        "mniNavUndeleteAndDisableReferences" | "mniNavUndeleteAndDisableReferencesObsolete" => true,
        _ => return None,
    };
    let obsolete = id.ends_with("Obsolete");
    Some(if allow {
        !obsolete && (!needs_check || check_for_errors)
    } else {
        obsolete && !hide && (!needs_check || check_for_errors)
    })
}

/// What the navigation popup needs to know about the focused element and the
/// selection to tell which items upstream shows.
pub struct NavContext<'a> {
    /// The focused node and the selection (`vstNav.FocusedNode` and
    /// `vstNav.GetSortedSelection(True)`).
    pub selection: &'a crate::selection::Selection,
    /// `wbEditAllowed`.
    pub edit_allowed: bool,
    /// The number of entries of `Container.GetAddList`.
    pub add_list_len: usize,
    /// `wbIsLightSupported`.
    pub light_supported: bool,
    /// The game of the session (`wbGameMode`).
    pub game: xedit_core::interface::globals::GameMode,
    /// Whether the session has mod groups (`wbModGroupsByName`).
    pub has_mod_groups: bool,
}

impl NavContext<'_> {
    /// The focused node of the selection.
    fn focused(&self) -> Option<&crate::selection::SelectedNode> {
        self.selection.focused.as_ref()
    }

    /// `mniNavCheckForErrors.Visible`: an element is focused and the
    /// session may be edited.
    fn check_for_errors(&self) -> bool {
        self.edit_allowed && self.focused().is_some()
    }

    /// Whether the focused node is a file (`Element.ElementType = etFile`).
    fn is_file(&self) -> bool {
        self.focused().is_some_and(|node| node.is_file())
    }

    /// Whether the focused node is a main record
    /// (`Element.ElementType = etMainRecord`).
    fn is_record(&self) -> bool {
        self.focused().is_some_and(|node| node.is_record())
    }

    /// `mniNavAddMasters.Visible` and the items that follow it.
    fn add_masters(&self) -> bool {
        self.check_for_errors() && self.is_file()
    }

    /// `mniNavCopyAsOverride.Visible`.
    fn copy_as_override(&self) -> bool {
        self.check_for_errors() && !self.add_masters()
    }

    /// The number of the selected main records that share the signature of
    /// the focused one (`mniNavCompareSelected`).
    pub fn compare_count(&self) -> usize {
        let Some(focused) = self.focused() else {
            return 0;
        };
        if !focused.is_record() {
            return 0;
        }
        self.selection
            .nodes
            .iter()
            .filter(|node| node.is_record() && node.signature == focused.signature)
            .count()
    }

    /// The number of the selected files (`mniNavCreateModGroup`).
    fn file_count(&self) -> usize {
        self.selection.nodes.iter().filter(|node| node.is_file()).count()
    }

    /// Whether the selection holds only `REFR` records
    /// (`SelectionIncludesOnlyREFR`).
    fn only_refr(&self) -> bool {
        !self.selection.nodes.is_empty()
            && self
                .selection
                .nodes
                .iter()
                .all(|node| node.is_record() && node.signature.as_deref() == Some("REFR"))
    }
}

/// Whether the navigation popup shows the item.
///
/// The rules are the ones of `TfrmMain.pmuNavPopup`
/// (`xeMainForm.pas:15410-15680`): upstream hides the items that do not
/// apply to the focused element, where the port would draw them greyed.
pub fn nav_item_visible(id: &str, ctx: &NavContext) -> bool {
    let focused = ctx.focused();
    let element = focused.is_some();
    let check_for_errors = ctx.check_for_errors();
    let renumber_from = ctx.edit_allowed && ctx.is_file() && focused.is_some_and(|node| node.editable);
    let copy_as_override = ctx.copy_as_override();
    let copy_as_wrapper = copy_as_override
        && ctx.is_record()
        && focused.is_some_and(|node| {
            matches!(
                node.signature.as_deref(),
                Some("LVLB" | "LVLC" | "LVLI" | "LVLN" | "LVLP" | "LVSC" | "LVSP")
            )
        });
    let is_skyrim = matches!(
        ctx.game,
        xedit_core::interface::globals::GameMode::gmTES5
            | xedit_core::interface::globals::GameMode::gmSSE
            | xedit_core::interface::globals::GameMode::gmEnderalSE
            | xedit_core::interface::globals::GameMode::gmTES5VR
    );
    let is_fo4 = matches!(
        ctx.game,
        xedit_core::interface::globals::GameMode::gmFO4 | xedit_core::interface::globals::GameMode::gmFO4VR
    );
    match id {
        // `mniNavTest.Visible := DebugHook <> 0` and the code site log of
        // a build without `USE_CODESITE`.
        "mniNavTest" | "mniNavOtherCodeSiteLogging" => false,
        // `mniNavHidden.Visible := Assigned(Element)`.
        "mniNavHidden" => element,
        // `xeMainForm.pas:15429`: a main record that is not the header of
        // a file and may be edited.
        "mniNavChangeFormID" => {
            ctx.edit_allowed
                && ctx.is_record()
                && focused.is_some_and(|node| node.editable)
                && focused.and_then(|node| node.signature.as_deref()) != Some("TES4")
        }
        // `xeMainForm.pas:15439`: a file that may be edited.
        "mniNavRenumberFormIDsFrom" => renumber_from,
        // `xeMainForm.pas:15446`: a file that is neither light nor an
        // update of a light master.
        "mniNavCompactFormIDs" => {
            renumber_from && ctx.light_supported && focused.is_some_and(|node| !node.is_light && !node.is_update)
        }
        // `xeMainForm.pas:15453`: a file with masters.
        "mniNavRenumberFormIDsInject" => renumber_from && focused.is_some_and(|node| node.master_count > 0),
        // `xeMainForm.pas:15458`: a record that is referred to.
        "mniNavChangeReferencingRecords" => {
            ctx.edit_allowed && ctx.is_record() && focused.is_some_and(|node| node.referenced_by_count > 0)
        }
        "mniNavCheckForErrors" | "mniNavCheckForCircularLeveledLists" => check_for_errors,
        // `mniNavSetVWDAuto.Visible := mniNavCheckForErrors.Visible and
        // wbIsOblivion`, and the same for `mniNavSetVWDAutoInto`.
        "mniNavSetVWDAuto" | "mniNavSetVWDAutoInto" => false,
        // `mniNavLOManagersDirtyInfo.Visible := mniNavCheckForErrors.Visible
        // and (Length(LOOTPluginInfos) <> 0)`.
        "mniNavLOManagersDirtyInfo" => false,
        // `xeMainForm.pas:15494`: the selection holds records that may be
        // removed (`RemovableSelection`).
        "mniNavRemove" => ctx.edit_allowed && ctx.selection.nodes.iter().any(|node| node.is_record()),
        // `xeMainForm.pas:15498`: the selection holds records that may be
        // edited (`EditableSelection`).
        "mniNavMarkModified" => ctx.edit_allowed && ctx.selection.nodes.iter().any(|node| node.editable),
        // `xeMainForm.pas:15505`: the focused element is a file.
        "mniNavCompareTo" | "mniNavCreateDeltaPatch" => ctx.is_file(),
        // `xeMainForm.pas:15510`: a file of an editable session.
        "mniNavAddMasters" | "mniNavSortMasters" | "mniNavCleanMasters" | "mniNavBatchChangeReferencingRecords" => {
            ctx.add_masters()
        }
        // `xeMainForm.pas:15515`.
        "mniNavApplyScript" => check_for_errors,
        // `xeMainForm.pas:15516`: the games that have a LOD.
        "mniNavGenerateLOD" => {
            ctx.is_file()
                && matches!(
                    ctx.game,
                    xedit_core::interface::globals::GameMode::gmTES4
                        | xedit_core::interface::globals::GameMode::gmFO3
                        | xedit_core::interface::globals::GameMode::gmFNV
                        | xedit_core::interface::globals::GameMode::gmTES5
                        | xedit_core::interface::globals::GameMode::gmSSE
                        | xedit_core::interface::globals::GameMode::gmEnderalSE
                        | xedit_core::interface::globals::GameMode::gmTES5VR
                        | xedit_core::interface::globals::GameMode::gmFO4
                        | xedit_core::interface::globals::GameMode::gmFO4VR
                        | xedit_core::interface::globals::GameMode::gmSF1
                )
        }
        // `xeMainForm.pas:15560`: the container of the focused element
        // takes new elements.
        "mniNavAdd" => ctx.add_list_len > 0,
        // `xeMainForm.pas:15544`: the copy of a record into another file.
        "mniNavCopyAsOverride" | "mniNavCopyAsOverrideWithOverwrite" => copy_as_override,
        // The deep copy needs a record that can be deep copied; the port
        // does not know which records those are, so the item stays hidden.
        "mniNavDeepCopyAsOverride" | "mniNavDeepCopyAsOverrideWithOverwriting" => false,
        "mniNavCopyAsNewRecord" => copy_as_override,
        "mniNavCopyAsWrapper" | "mniNavCopyAsSpawnRateOverride" => copy_as_wrapper,
        // `xeMainForm.pas:15545`: the idle animations of the games before
        // Skyrim.
        "mniNavCopyIdle" => false,
        // `xeMainForm.pas:15547`: a record that refers to an injected one.
        "mniNavCleanupInjected" => false,
        // `xeMainForm.pas:15582`: more than one record of the same
        // signature is selected.
        "mniNavCompareSelected" => ctx.compare_count() > 1,
        // `xeMainForm.pas:15607`: more than one file is selected.
        "mniNavCreateModGroup" => ctx.file_count() > 1,
        // `xeMainForm.pas:15622`: the session has mod groups.
        "mniNavEditModGroup" | "mniNavDeleteModGroups" | "mniNavUpdateCRCModGroups" => ctx.has_mod_groups,
        // `xeMainForm.pas:15626`: the temporary and the persistent cell
        // children are never shown.
        "mniNavCellChildPers" | "mniNavCellChildTemp" => false,
        // `xeMainForm.pas:15627`: the selection holds only references.
        "mniNavCellChildNotVWD" | "mniNavCellChildVWD" => check_for_errors && ctx.only_refr(),
        // `xeMainForm.pas:15636`: the sequence file of Skyrim.
        "mniNavCreateSEQFile" => is_skyrim && ctx.is_file(),
        // `xeMainForm.pas:15643`.
        "mniNavLocalization" => is_skyrim || is_fo4,
        // `xeMainForm.pas:15644`: a file of the load order.
        "mniNavLocalizationSwitch" => ctx.is_file() && focused.is_some_and(|node| node.load_order > 0),
        // `xeMainForm.pas:15655`.
        "mniNavLogAnalyzer" => {
            is_skyrim
                || matches!(
                    ctx.game,
                    xedit_core::interface::globals::GameMode::gmTES4
                        | xedit_core::interface::globals::GameMode::gmFO3
                        | xedit_core::interface::globals::GameMode::gmFNV
                )
        }
        // `xeMainForm.pas:15668`.
        "mniNavCreateMergedPatch" => ctx.edit_allowed,
        _ => true,
    }
}
