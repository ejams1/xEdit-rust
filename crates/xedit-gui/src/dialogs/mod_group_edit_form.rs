// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeModGroupEditForm.pas (FormCreate, FormShow,
// FormKeyDown, FormClose, ShowModal's CRC32 question, CheckState,
// TagContainedModules, vstModGroupItemsInitNode,
// vstModGroupItemsInitChildren, vstModGroupItemsGetText,
// vstModGroupItemsKeyDown, vstModGroupItemsNodeClick, edNameChange,
// edNameKeyDown) and xEdit/xeModGroupEditForm.dfm

//! `TfrmModGroupEdit`: the items of one mod group as an editable tree —
//! the six flag columns, the CRC32 children, Insert of a module through the
//! module selection dialog or of a CRC32 through the `InputQuery`, Delete,
//! the Ctrl+Up/Down order, Ctrl+C, Ctrl+N and the name box with
//! `CheckState`.

use eframe::egui;

use xedit_loadorder::mod_groups::{ItemFlags, ModGroupItem, crc32_from_string};
use xedit_session::modgroups::ShowResponse;
use xedit_session::module_select::ModulesListResponse;

use crate::dialogs::module_select_form::{self, ModuleSelectAction};
use crate::dialogs::selection::{DialogPlacement, Question, show_question};
use crate::startup::{ModuleSelectMode, ModuleSelectState};
use crate::widgets::conflict::ConflictColors;
use crate::widgets::virtual_tree::{TreeCell, TreeColumn, TreeKeyAction, TreeRow, TreeSource, TreeState, TreeView};
use xedit_core::interface::types::{ConflictAll, ConflictThis};

/// The columns of `vstModGroupItems` in `xeModGroupEditForm.dfm`.
pub const EDIT_COLUMNS: [(&str, f32); 7] = [
    ("Name", 424.0),
    ("Optional", 70.0),
    ("Target", 70.0),
    ("Source", 70.0),
    ("Forbidden", 70.0),
    ("Ignore LO", 70.0),
    ("Load Order", 70.0),
];

/// What the dialog asks the shell to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModGroupEditAction {
    /// `mrOk`: the shell sends `modgroups.edit` with
    /// [`ModGroupEditState::lines`], the new name and the CRC answer.
    Ok,
    Cancelled,
}

/// One item of the group, with the file facts `modgroups.show` reports.
#[derive(Clone, Debug, PartialEq)]
pub struct EditItem {
    pub flags: ItemFlags,
    pub file_name: String,
    pub crc32s: Vec<u32>,
    pub current_crc32: Option<u32>,
    pub load_order: Option<i32>,
    /// `vstModGroupItemsInitChildren`: the CRC32s are shown.
    pub expanded: bool,
}

impl EditItem {
    /// The item of `modgroups.show`.
    pub fn from_info(info: &xedit_session::modgroups::ItemInfo) -> Self {
        Self {
            flags: ItemFlags {
                optional: info.optional,
                is_target: info.target,
                is_source: info.source,
                forbidden: info.forbidden,
                ignore_load_order_always: info.ignore_load_order,
                ignore_load_order_in_block: info.ignore_load_order_in_block,
                valid: info.valid,
                has_file: info.has_file,
            },
            file_name: info.file_name.clone(),
            crc32s: info.crc32s.iter().filter_map(|crc| crc32_from_string(crc)).collect(),
            current_crc32: info.current_crc32.as_deref().and_then(crc32_from_string),
            load_order: info.load_order,
            expanded: false,
        }
    }

    /// `TwbModGroupItem.ToString`: the line `modgroups.edit` takes.
    pub fn line(&self) -> String {
        ModGroupItem {
            flags: self.flags,
            file_name: self.file_name.clone(),
            module: None,
            crc32s: self.crc32s.clone(),
            valid_msgs: Vec::new(),
        }
        .to_string()
    }

    /// `mgifHasFile`: the module is loaded and matches the CRC32s.
    pub fn has_file(&self) -> bool {
        self.flags.has_file
    }
}

/// One visible row: an item or one of its CRC32s.
#[derive(Clone, Debug, PartialEq)]
pub struct EditRow {
    pub row: usize,
    pub item: usize,
    pub crc: Option<usize>,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub text: String,
    pub optional: String,
    pub target: String,
    pub source: String,
    pub forbidden: String,
    pub ignore_load_order: String,
    pub load_order: String,
    /// The CRC32 radio of a CRC32 row is checked for the current file.
    pub checked: bool,
    /// `mfHasFile` missing on the item: struck out upstream.
    pub invalid: bool,
}

/// The question of the form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditQuestion {
    /// `FormShow`'s "The following entries have CRC32s, but don't contain
    /// the CRC of the current file: ...". The default is No.
    AddCurrentCrcs(Vec<String>),
    /// `VK_DELETE` on an item: "Do you want to delete ... from this
    /// ModGroup?" The default is No.
    DeleteItem(usize),
    /// `VK_DELETE` on a CRC32: "Do you want to delete CRC32 ... from
    /// ...?" The default is No.
    DeleteCrc { item: usize, crc: usize },
}

/// The `InputQuery('New file CRC32', ...)` of `VK_INSERT`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrcInput {
    pub item: usize,
    pub text: String,
    pub error: Option<String>,
}

/// `TfrmModGroupEdit` as data.
pub struct ModGroupEditState {
    pub name: String,
    pub original_name: String,
    /// The `.modgroups` file of the group.
    pub file: Option<String>,
    pub items: Vec<EditItem>,
    /// `pnlError`'s text without the `Error: ` prefix.
    pub error: Option<String>,
    pub caption: String,
    /// The answer to the `FormShow` CRC32 question.
    pub add_current_crcs: bool,
    pub question: Option<EditQuestion>,
    pub crc_input: Option<CrcInput>,
    /// `wbModulesByLoadOrder` for the Insert dialog.
    pub modules: ModulesListResponse,
    /// The nested `TfrmModuleSelect` of `VK_INSERT`.
    pub module_select: Option<ModuleSelectState>,
    pub rows: Vec<EditRow>,
    pub tree: TreeState,
    pub placement: Option<DialogPlacement>,
    pub rect: Option<egui::Rect>,
}

impl ModGroupEditState {
    /// `ShowModal`'s setup: the items, the name and the CRC32 question.
    pub fn new(show: &ShowResponse, modules: ModulesListResponse, caption: impl Into<String>) -> Self {
        let items: Vec<EditItem> = show.mod_group.items.iter().map(EditItem::from_info).collect();
        let missing: Vec<String> = items
            .iter()
            .filter(|item| !item.crc32s.is_empty() && item.current_crc32.is_some_and(|crc| !item.crc32s.contains(&crc)))
            .map(|item| item.file_name.clone())
            .collect();
        let mut state = Self {
            name: show.mod_group.name.clone(),
            original_name: show.mod_group.name.clone(),
            file: Some(show.mod_group.file.clone()),
            items,
            error: None,
            caption: caption.into(),
            add_current_crcs: false,
            question: (!missing.is_empty()).then_some(EditQuestion::AddCurrentCrcs(missing)),
            crc_input: None,
            modules,
            module_select: None,
            rows: Vec::new(),
            tree: TreeState::new(EDIT_COLUMNS.iter().map(|(_, width)| *width).collect()),
            placement: None,
            rect: None,
        };
        state.check_state();
        state
    }

    /// The state of the tests, without a command response.
    pub fn for_test(name: &str, items: Vec<EditItem>) -> Self {
        let mut state = Self {
            name: name.to_owned(),
            original_name: name.to_owned(),
            file: None,
            items,
            error: None,
            caption: String::new(),
            add_current_crcs: false,
            question: None,
            crc_input: None,
            modules: ModulesListResponse {
                data_path: String::new(),
                modules: Vec::new(),
            },
            module_select: None,
            rows: Vec::new(),
            tree: TreeState::new(EDIT_COLUMNS.iter().map(|(_, width)| *width).collect()),
            placement: None,
            rect: None,
        };
        state.check_state();
        state
    }

    /// `CheckState`: the name and the item count.
    pub fn check_state(&mut self) {
        self.error = if self.name.is_empty() {
            Some("Name is empty".to_owned())
        } else if self.items.len() < 2 {
            Some("ModGroup needs to contain at least 2 items".to_owned())
        } else {
            None
        };
        self.refresh_rows();
    }

    /// `btnOK.Enabled`.
    pub fn ok_enabled(&self) -> bool {
        self.error.is_none()
    }

    /// The rows `InitNode`/`InitChildren` show.
    pub fn refresh_rows(&mut self) {
        let mut rows = Vec::new();
        for (item_index, item) in self.items.iter().enumerate() {
            rows.push(EditRow {
                row: rows.len(),
                item: item_index,
                crc: None,
                depth: 0,
                has_children: !item.crc32s.is_empty(),
                expanded: item.expanded,
                text: item.file_name.clone(),
                optional: if item.flags.optional { "Optional" } else { "" }.to_owned(),
                target: if item.flags.is_target { "Target" } else { "" }.to_owned(),
                source: if item.flags.is_source { "Source" } else { "" }.to_owned(),
                forbidden: if item.flags.forbidden { "Forbidden" } else { "" }.to_owned(),
                ignore_load_order: if item.flags.ignore_load_order_always {
                    "Always"
                } else if item.flags.ignore_load_order_in_block {
                    "in Block"
                } else {
                    ""
                }
                .to_owned(),
                load_order: item.load_order.map(|order| order.to_string()).unwrap_or_default(),
                checked: false,
                invalid: !item.has_file(),
            });
            if !item.expanded {
                continue;
            }
            for (crc_index, crc) in item.crc32s.iter().enumerate() {
                rows.push(EditRow {
                    row: rows.len(),
                    item: item_index,
                    crc: Some(crc_index),
                    depth: 1,
                    has_children: false,
                    expanded: false,
                    text: format!("{crc:08X}"),
                    optional: String::new(),
                    target: String::new(),
                    source: String::new(),
                    forbidden: String::new(),
                    ignore_load_order: String::new(),
                    load_order: String::new(),
                    checked: item.current_crc32 == Some(*crc),
                    invalid: false,
                });
            }
        }
        self.rows = rows;
    }

    /// `vstModGroupItemsNodeClick`: columns 1..5 toggle a flag of an item.
    pub fn click_row(&mut self, row: usize, column: usize) {
        let Some(edit_row) = self.rows.get(row) else {
            return;
        };
        if edit_row.crc.is_some() {
            return;
        }
        let item = edit_row.item;
        let flags = &mut self.items[item].flags;
        match column {
            1 => {
                if flags.optional {
                    flags.optional = false;
                } else {
                    flags.optional = true;
                    flags.forbidden = false;
                }
            }
            2 => flags.is_target = !flags.is_target,
            3 => flags.is_source = !flags.is_source,
            4 => {
                if flags.forbidden {
                    flags.forbidden = false;
                } else {
                    flags.forbidden = true;
                    flags.optional = false;
                }
            }
            5 => {
                if flags.ignore_load_order_always {
                    flags.ignore_load_order_always = false;
                    flags.ignore_load_order_in_block = true;
                } else if flags.ignore_load_order_in_block {
                    flags.ignore_load_order_in_block = false;
                } else {
                    flags.ignore_load_order_always = true;
                }
            }
            _ => {}
        }
        self.check_state();
    }

    /// `toToggleOnDblClick`.
    pub fn toggle_expanded(&mut self, row: usize) -> bool {
        let Some(edit_row) = self.rows.get(row) else {
            return false;
        };
        if edit_row.crc.is_some() || !edit_row.has_children {
            return false;
        }
        let item = edit_row.item;
        self.items[item].expanded = !self.items[item].expanded;
        self.refresh_rows();
        true
    }

    /// `VK_DELETE`: the question unless Shift answered it, then the node
    /// goes.
    pub fn delete_row(&mut self, row: usize, skip_question: bool) {
        let Some(edit_row) = self.rows.get(row) else {
            return;
        };
        let (item, crc) = (edit_row.item, edit_row.crc);
        if skip_question {
            self.apply_delete(item, crc);
            return;
        }
        self.question = Some(match crc {
            Some(crc) => EditQuestion::DeleteCrc { item, crc },
            None => EditQuestion::DeleteItem(item),
        });
    }

    fn apply_delete(&mut self, item: usize, crc: Option<usize>) {
        match crc {
            Some(crc) => {
                if let Some(entry) = self.items.get_mut(item)
                    && crc < entry.crc32s.len()
                {
                    entry.crc32s.remove(crc);
                }
            }
            None => {
                if item < self.items.len() {
                    self.items.remove(item);
                }
            }
        }
        self.check_state();
        let rows = self.rows.len();
        self.tree.clamp(rows);
    }

    /// `VK_INSERT`: a CRC32 of the focused item (or with Shift on an item
    /// row), otherwise the module selection dialog.
    pub fn insert(&mut self, row: Option<usize>, shift: bool) {
        let row = row.and_then(|row| self.rows.get(row));
        let item = match row {
            Some(row) if row.crc.is_some() || shift => Some(row.item),
            _ => None,
        };
        if let Some(item) = item {
            let current = self.items[item].current_crc32;
            let duplicate = current.is_some_and(|crc| self.items[item].crc32s.contains(&crc));
            let text = current
                .filter(|_| !duplicate)
                .map(|crc| format!("{crc:08X}"))
                .unwrap_or_default();
            self.crc_input = Some(CrcInput {
                item,
                text,
                error: None,
            });
        } else {
            self.open_module_select();
        }
    }

    /// The `InputQuery` answered: validate, then add the CRC32.
    pub fn submit_crc_input(&mut self) {
        let Some(input) = self.crc_input.clone() else {
            return;
        };
        let Some(crc) = crc32_from_string(input.text.trim()) else {
            if let Some(input) = &mut self.crc_input {
                input.error = Some("The entered text was not a valid CRC32. Please try again.".to_owned());
            }
            return;
        };
        if self.items[input.item].crc32s.contains(&crc) {
            if let Some(input) = &mut self.crc_input {
                input.error = Some("The entered CRC32 has already been previously added. Please try again.".to_owned());
            }
            return;
        }
        self.items[input.item].crc32s.push(crc);
        self.crc_input = None;
        self.check_state();
    }

    /// `VK_INSERT` without a CRC32 row: `TfrmModuleSelect` with
    /// `FilterFlag = mfEphemeralModGroupTagged` and the group's modules
    /// tagged out (`TagContainedModules(True)`).
    pub fn open_module_select(&mut self) {
        let contained: Vec<String> = self.items.iter().map(|item| item.file_name.clone()).collect();
        let state = ModuleSelectState::from_modules_list(self.modules.clone(), ModuleSelectMode::Insert, &contained);
        self.module_select = Some(state);
    }

    /// The module chosen in the Insert dialog: its item becomes a target
    /// and a source, inserted by load order.
    pub fn add_module(&mut self, name: &str) {
        let Some(module) = self
            .modules
            .modules
            .iter()
            .find(|module| module.name.eq_ignore_ascii_case(name))
        else {
            return;
        };
        let item = EditItem {
            flags: ItemFlags {
                is_target: true,
                is_source: true,
                ..ItemFlags::default()
            },
            file_name: module.name.clone(),
            crc32s: Vec::new(),
            current_crc32: module.crc32,
            load_order: module.load_order,
            expanded: false,
        };
        let position = self
            .items
            .iter()
            .position(|existing| {
                existing
                    .load_order
                    .zip(item.load_order)
                    .is_some_and(|(existing, new)| existing > new)
            })
            .unwrap_or(self.items.len());
        self.items.insert(position, item);
        self.check_state();
    }

    /// Ctrl+Up / Ctrl+Down: the item moves among its siblings.
    pub fn move_item(&mut self, row: usize, up: bool) {
        let Some(edit_row) = self.rows.get(row) else {
            return;
        };
        if edit_row.crc.is_some() {
            return;
        }
        let item = edit_row.item;
        let target = if up { item.checked_sub(1) } else { Some(item + 1) };
        let Some(target) = target.filter(|target| *target < self.items.len()) else {
            return;
        };
        self.items.swap(item, target);
        self.check_state();
        // Keep the focus on the moved item.
        let row = self.rows.iter().position(|row| row.item == target && row.crc.is_none());
        if let Some(row) = row {
            self.tree.select_only(row);
        }
    }

    /// `Ctrl+C`: the file name or the CRC32 of the focused row.
    pub fn clipboard_text(&self, row: Option<usize>) -> Option<String> {
        let row = self.rows.get(row?)?;
        match row.crc {
            Some(crc) => Some(format!("{:08X}", self.items[row.item].crc32s[crc])),
            None => Some(self.items[row.item].file_name.clone()),
        }
    }

    /// `Ctrl+N`: the name box takes the focused item's file name without
    /// its extension (`ChangeFileExt(..., '')`).
    pub fn name_from_row(&mut self, row: Option<usize>) {
        let Some(row) = row.and_then(|row| self.rows.get(row)) else {
            return;
        };
        if row.crc.is_some() {
            return;
        }
        let file_name = &self.items[row.item].file_name;
        self.name = match file_name.rfind('.') {
            Some(at) => file_name[..at].to_owned(),
            None => file_name.clone(),
        };
        self.check_state();
    }

    /// The lines `modgroups.edit` takes.
    pub fn lines(&self) -> Vec<String> {
        self.items.iter().map(EditItem::line).collect()
    }

    /// The `modgroups.edit` request's CRC answer.
    pub fn add_current_crcs(&self) -> bool {
        self.add_current_crcs
    }
}

/// The tree adapter.
struct EditTree<'a> {
    rows: &'a [EditRow],
    columns: Vec<TreeColumn>,
}

impl<'a> EditTree<'a> {
    fn new(rows: &'a [EditRow]) -> Self {
        Self {
            rows,
            columns: EDIT_COLUMNS
                .iter()
                .map(|(title, width)| TreeColumn::new(*title, *width))
                .collect(),
        }
    }
}

impl TreeSource for EditTree<'_> {
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn columns(&self) -> &[TreeColumn] {
        &self.columns
    }

    fn row(&self, index: usize, out: &mut TreeRow) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        out.id = row.row as u64;
        out.depth = row.depth;
        out.has_children = row.has_children;
        out.expanded = row.expanded;
        if row.crc.is_some() {
            out.icon = Some(if row.checked { '\u{25C9}' } else { '\u{25CB}' });
        }
        out.cells = vec![
            TreeCell::new(row.text.clone()),
            TreeCell::new(row.optional.clone()),
            TreeCell::new(row.target.clone()),
            TreeCell::new(row.source.clone()),
            TreeCell::new(row.forbidden.clone()),
            TreeCell::new(row.ignore_load_order.clone()),
            TreeCell::new(row.load_order.clone()),
        ];
        if row.invalid {
            out.cells[0].tooltip = Some("The item has no loaded file".to_owned());
            out.conflict_all = ConflictAll::caConflict;
            out.conflict_this = ConflictThis::ctConflictLoses;
        }
    }
}

/// Draws the dialog.
pub fn show(ctx: &egui::Context, state: &mut ModGroupEditState) -> Option<ModGroupEditAction> {
    // The Insert dialog is modal: while it is open the edit form is not.
    if state.module_select.is_some() {
        let action = {
            let select = state.module_select.as_mut().expect("checked above");
            module_select_form::show(ctx, select)
        };
        match action {
            Some(ModuleSelectAction::Ok) => {
                let names = state
                    .module_select
                    .as_mut()
                    .and_then(|select| select.selected_names().ok())
                    .unwrap_or_default();
                state.module_select = None;
                if let Some(name) = names.first() {
                    state.add_module(name);
                }
            }
            Some(ModuleSelectAction::Cancelled) => state.module_select = None,
            Some(_) | None => {}
        }
        return None;
    }

    let mut action = None;
    let mut open = true;
    let mut name_focused = false;
    let mut clicked = None;
    let mut double_clicked = None;
    let mut expand_row = None;
    let mut window = egui::Window::new(if state.caption.is_empty() {
        "ModGroup Selection"
    } else {
        &state.caption
    })
    .id(egui::Id::unique("frmModGroupEdit"))
    .collapsible(false)
    .resizable(true)
    .default_size(crate::dialogs::selection::fit_size(ctx, egui::vec2(864.0, 615.0)))
    .open(&mut open);
    window = match state.placement {
        Some(placement) => placement.apply(ctx, window),
        None => window.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)),
    };
    let response = window.show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Name:");
            let edit = ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(600.0));
            name_focused = edit.has_focus();
            if edit.changed() {
                state.check_state();
            }
        });
        ui.separator();
        let list_height = (ui.available_height() - 34.0).max(80.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), list_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                let mut source = EditTree::new(&state.rows);
                let output = TreeView {
                    id: ui.scope_id().with("modgroup-items"),
                    source: &mut source,
                    state: &mut state.tree,
                    colors: &ConflictColors::default(),
                    header: true,
                }
                .show(ui);
                clicked = output.clicked.map(|row| (row, output.clicked_column));
                double_clicked = output.double_clicked;
                expand_row = output.expander_clicked;
                if let Some(TreeKeyAction::Toggle(row)) = output.key {
                    expand_row = Some(row);
                }
                if let Some(TreeKeyAction::Activate(row)) = output.key {
                    expand_row = Some(row);
                }
            },
        );
        if let Some(error) = &state.error {
            ui.colored_label(ui.visuals().error_fg_color, format!("Error: {error}"));
        } else {
            ui.label("");
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(state.ok_enabled(), egui::Button::new("OK")).clicked() {
                state.check_state();
                if state.ok_enabled() {
                    action = Some(ModGroupEditAction::Ok);
                }
            }
            if ui.button("Cancel").clicked() {
                action = Some(ModGroupEditAction::Cancelled);
            }
        });
    });
    if let Some(response) = response {
        state.rect = Some(response.response.rect);
    }

    if let Some((row, column)) = clicked
        && let Some(column) = column
    {
        state.click_row(row, column);
    }
    if let Some(row) = double_clicked.or(expand_row) {
        state.toggle_expanded(row);
    }

    // `vstModGroupItemsKeyDown` and `FormKeyDown`.
    let typing = name_focused || state.crc_input.is_some();
    if action.is_none() && !typing && state.question.is_none() {
        let modifiers = ctx.input(|input| input.modifiers);
        if ctx.input(|input| input.key_pressed(egui::Key::Delete)) {
            if let Some(row) = state.tree.focused {
                state.delete_row(row, modifiers.shift);
            }
        } else if ctx.input(|input| input.key_pressed(egui::Key::Insert)) {
            state.insert(state.tree.focused, modifiers.shift);
        } else if modifiers.command && ctx.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
            if let Some(row) = state.tree.focused {
                state.move_item(row, true);
            }
        } else if modifiers.command && ctx.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
            if let Some(row) = state.tree.focused {
                state.move_item(row, false);
            }
        } else if modifiers.command && ctx.input(|input| input.key_pressed(egui::Key::C)) {
            if let Some(text) = state.clipboard_text(state.tree.focused) {
                ctx.copy_text(text);
            }
        } else if modifiers.command && ctx.input(|input| input.key_pressed(egui::Key::N)) {
            let row = state.tree.focused;
            state.name_from_row(row);
        } else if crate::dialogs::selection::char_pressed(ctx, ' ') {
            if let Some(row) = state.tree.focused {
                state.click_row(row, state.tree.focused_column);
            }
        } else if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
            state.check_state();
            if state.ok_enabled() {
                action = Some(ModGroupEditAction::Ok);
            }
        } else if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            action = Some(ModGroupEditAction::Cancelled);
        }
    }

    // The `FormShow` question and the delete questions.
    if let Some(question) = state.question.clone() {
        let (text, default_yes) = match &question {
            EditQuestion::AddCurrentCrcs(files) => (
                format!(
                    "The following entries have CRC32s, but don't contain the CRC of the current file:\n\n{}\n\nDo you want to add the current CRC32s to these entries?",
                    files.join("\r\n")
                ),
                false,
            ),
            EditQuestion::DeleteItem(item) => (
                format!(
                    "Do you want to delete \"{}\" from this ModGroup?",
                    state.items.get(*item).map(|item| item.file_name.as_str()).unwrap_or("")
                ),
                false,
            ),
            EditQuestion::DeleteCrc { item, crc } => (
                format!(
                    "Do you want to delete CRC32 \"{:08X}\" from \"{}\"?",
                    state
                        .items
                        .get(*item)
                        .and_then(|item| item.crc32s.get(*crc))
                        .copied()
                        .unwrap_or(0),
                    state.items.get(*item).map(|item| item.file_name.as_str()).unwrap_or("")
                ),
                false,
            ),
        };
        match show_question(ctx, "modgroup-edit-question", &Question::new(text, default_yes)) {
            Some(true) => {
                state.question = None;
                match question {
                    EditQuestion::AddCurrentCrcs(_) => state.add_current_crcs = true,
                    EditQuestion::DeleteItem(item) => state.apply_delete(item, None),
                    EditQuestion::DeleteCrc { item, crc } => state.apply_delete(item, Some(crc)),
                }
            }
            Some(false) => {
                state.question = None;
                if let EditQuestion::AddCurrentCrcs(_) = question {
                    state.add_current_crcs = false;
                }
            }
            None => return None,
        }
    }

    // The `InputQuery('New file CRC32', ...)`.
    if state.crc_input.is_some() {
        let mut answer = None;
        let mut cancel = false;
        let mut open_input = true;
        egui::Window::new("New file CRC32")
            .id(egui::Id::unique("modgroup-edit-crc-input"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .open(&mut open_input)
            .show(ctx, |ui| {
                ui.label("Please enter the to be added CRC32 as an 8 digit Hex number, e.g 1A2B3C4D");
                if let Some(input) = &mut state.crc_input {
                    ui.add(egui::TextEdit::singleline(&mut input.text).desired_width(160.0));
                    if let Some(error) = &input.error {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if answer == Some(true) || ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
            state.submit_crc_input();
        } else if cancel || !open_input {
            state.crc_input = None;
        }
        if state.crc_input.is_some() {
            return None;
        }
    }

    if !open && action.is_none() {
        action = Some(ModGroupEditAction::Cancelled);
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use xedit_session::module_select::ModuleEntry;

    fn item(name: &str, target: bool, source: bool) -> EditItem {
        EditItem {
            flags: ItemFlags {
                is_target: target,
                is_source: source,
                has_file: true,
                valid: true,
                ..ItemFlags::default()
            },
            file_name: name.to_owned(),
            crc32s: Vec::new(),
            current_crc32: None,
            load_order: Some(0),
            expanded: false,
        }
    }

    fn state() -> ModGroupEditState {
        ModGroupEditState::for_test("MyGroup", vec![item("A.esp", true, false), item("B.esp", false, true)])
    }

    #[test]
    fn the_check_state_names_the_two_errors() {
        let mut state = ModGroupEditState::for_test("", vec![item("A.esp", true, true)]);
        assert!(!state.ok_enabled());
        assert_eq!(state.error.as_deref(), Some("Name is empty"));
        state.name = "Name".to_owned();
        state.check_state();
        assert_eq!(
            state.error.as_deref(),
            Some("ModGroup needs to contain at least 2 items")
        );
        state.items.push(item("B.esp", true, true));
        state.check_state();
        assert!(state.ok_enabled());
    }

    #[test]
    fn the_flag_columns_toggle_as_upstream() {
        let mut state = state();
        state.click_row(0, 1);
        assert!(state.items[0].flags.optional);
        state.click_row(0, 4);
        assert!(state.items[0].flags.forbidden);
        assert!(!state.items[0].flags.optional);
        state.click_row(0, 2);
        assert!(!state.items[0].flags.is_target);
        state.click_row(0, 3);
        assert!(state.items[0].flags.is_source);
        state.click_row(0, 5);
        assert!(state.items[0].flags.ignore_load_order_always);
        state.click_row(0, 5);
        assert!(state.items[0].flags.ignore_load_order_in_block);
        state.click_row(0, 5);
        assert!(!state.items[0].flags.ignore_load_order_in_block);
    }

    #[test]
    fn a_deleted_item_needs_at_least_two_left() {
        let mut state = state();
        state.delete_row(0, true);
        assert_eq!(state.items.len(), 1);
        // `CheckState` reports the count.
        assert_eq!(
            state.error.as_deref(),
            Some("ModGroup needs to contain at least 2 items")
        );
        state.delete_row(0, false);
        assert!(state.question.is_some());
        assert_eq!(state.items.len(), 1);
    }

    #[test]
    fn a_crc_input_validates_and_refuses_duplicates() {
        let mut state = state();
        state.items[0].crc32s.push(0x1A2B_3C4D);
        state.insert(Some(0), true);
        let input = state.crc_input.clone().unwrap();
        assert_eq!(input.item, 0);
        state.crc_input.as_mut().unwrap().text = "nope".to_owned();
        state.submit_crc_input();
        assert!(
            state
                .crc_input
                .as_ref()
                .unwrap()
                .error
                .as_deref()
                .unwrap()
                .contains("not a valid CRC32")
        );
        state.crc_input.as_mut().unwrap().text = "1A2B3C4D".to_owned();
        state.submit_crc_input();
        assert!(
            state
                .crc_input
                .as_ref()
                .unwrap()
                .error
                .as_deref()
                .unwrap()
                .contains("already been")
        );
        state.crc_input.as_mut().unwrap().text = "FFFFFFFF".to_owned();
        state.submit_crc_input();
        assert!(state.crc_input.is_none());
        assert_eq!(state.items[0].crc32s, vec![0x1A2B_3C4D, 0xFFFF_FFFF]);
    }

    #[test]
    fn an_inserted_module_goes_in_load_order_and_is_added_after_its_masters() {
        let mut state = state();
        state.items[0].load_order = Some(1);
        state.items[1].load_order = Some(3);
        state.modules.modules.push(ModuleEntry {
            index: 0,
            name: "M.esp".to_owned(),
            original_name: "M.esp".to_owned(),
            load_order: Some(2),
            file_id: None,
            path: String::new(),
            size: None,
            valid: true,
            loaded: false,
            has_esm_flag: false,
            is_esm: false,
            has_esm_extension: false,
            is_light: false,
            is_medium: false,
            is_update: false,
            is_localized: false,
            has_blueprint: false,
            is_ghost: false,
            is_hardcoded: false,
            masters_missing: false,
            active: false,
            active_in_plugins_txt: false,
            has_index: false,
            date_time: 0.0,
            official_index: None,
            cc_index: None,
            plugins_txt_index: None,
            load_order_txt_index: None,
            masters: Vec::new(),
            crc32: None,
        });
        state.add_module("m.esp");
        assert_eq!(
            state
                .items
                .iter()
                .map(|item| item.file_name.clone())
                .collect::<Vec<_>>(),
            vec!["A.esp", "M.esp", "B.esp"]
        );
        assert!(state.items[1].flags.is_target && state.items[1].flags.is_source);
    }

    #[test]
    fn ctrl_up_and_down_move_the_item() {
        let mut state = state();
        state.move_item(1, true);
        assert_eq!(
            state
                .items
                .iter()
                .map(|item| item.file_name.clone())
                .collect::<Vec<_>>(),
            vec!["B.esp", "A.esp"]
        );
        state.move_item(0, false);
        assert_eq!(
            state
                .items
                .iter()
                .map(|item| item.file_name.clone())
                .collect::<Vec<_>>(),
            vec!["A.esp", "B.esp"]
        );
    }

    #[test]
    fn the_lines_are_the_modgroups_file_shape() {
        let mut state = state();
        state.items[0].flags.forbidden = true;
        state.items[1].flags.optional = true;
        state.items[1].crc32s.push(0xDEAD_BEEF);
        assert_eq!(state.lines(), vec!["!@A.esp".to_owned(), "+#B.esp:DEADBEEF".to_owned()]);
    }

    #[test]
    fn ctrl_n_takes_the_file_name_without_the_extension() {
        let mut state = state();
        state.name_from_row(Some(1));
        assert_eq!(state.name, "B");
        assert!(state.ok_enabled());
    }

    #[test]
    fn the_crc_rows_are_checked_for_the_current_file() {
        let mut state = state();
        state.items[0].crc32s = vec![0x1A2B_3C4D, 0xFFFF_FFFF];
        state.items[0].current_crc32 = Some(0x1A2B_3C4D);
        state.items[0].expanded = true;
        state.refresh_rows();
        assert_eq!(state.rows.len(), 4);
        assert!(state.rows[1].checked);
        assert!(!state.rows[2].checked);
    }
}
