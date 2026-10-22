// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (ApplyReferencedByFilter,
// ApplyReferencedByAlphaSort, lvReferencedByColumnClick,
// lvReferencedByLoadData, lvReferencedByDblClick, lvReferencedByKeyDown,
// lvReferencedByOnDataStateChange, lvReferencedByOnSelect,
// UpdateReferencedByTabCaption, PopulateReferencedByListData,
// ClearReferencedByListData, pgMainChange's lazy population,
// mniRefByCompareSelectedClick, mniRefByCopyIntoClick,
// mniRefByCopyDisabledOverrideIntoClick, mniRefByRemoveClick,
// mniRefByMarkModifiedClick, mniRefByVWDClick, mmoMessagesDblClick,
// mniMessagesClearClick, mniMessagesSaveSelectedClick, AddMessage,
// ScrollToTheLastMessage, tbsMessagesShow, tbsSpreadsheetShow,
// vstSpreadSheetGetText, vstSpreadSheetWeaponInitNode,
// vstSpreadsheetArmorInitNode, vstSpreadSheetAmmoInitNode,
// vstSpreadSheetNewText, vstSpreadSheetClick, mniSpreadsheetRebuildClick,
// mniSpreadsheetCompareSelectedClick, mniViewEditClick's localization
// branch) and xEdit/xeMainForm.dfm (lvReferencedBy, fpnlReferencedByFilter,
// mmoMessages, Memo1, tbsWEAPSpreadsheet, tbsARMOSpreadsheet,
// tbsAMMOSpreadsheet, pmuRefBy, pmuMessages, pmuSpreadsheet)

//! The record panes of `pgMain`: the Referenced By list with its filter
//! strip, the message log, the Information memo, the localization view of
//! the selected record and the three spreadsheets. The element detail
//! editor (`xeElementDetailForm`'s rules) is [`crate::element_detail`].
//!
//! Every pane reads through the [CommandBridge] and changes nothing of its
//! own: the rows of the Referenced By list are `refs.get`, the localization
//! view is `localization.get` over the record's elements, the spreadsheets
//! are `records.list` plus `records.get`, and the menu commands are the ones
//! `coverage/ledger.toml` binds to the DFM handlers. The models are pure
//! state so the filters, sorting and captions are tested without an egui
//! context.

use std::collections::BTreeSet;

use crate::command::CommandBridge;
use eframe::egui;
use serde_json::{Value, json};
use xedit_core::localization::compare_text;
use xedit_session::CommandError;
use xedit_session::commands::{
    ElementsSetRequest, ElementsSetResponse, RecordDetail, RecordList, RecordSummary, RecordsGetRequest,
    RecordsListRequest,
};
use xedit_session::formids::RecordRef;
use xedit_session::refs::{RefsGetRequest, RefsGetResponse};

use crate::widgets::grid::{Grid, GridCell, GridColumn, GridSource, GridState};

/// The record the panes follow: the active record of the view
/// (`DoSetActiveRecord`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordSelection {
    /// Load order FormID as hexadecimal digits.
    pub form_id: String,
    /// Plugin the record is seen from; the last loaded when `None`.
    pub file: Option<String>,
}

impl RecordSelection {
    pub fn new(form_id: impl Into<String>, file: Option<String>) -> Self {
        Self {
            form_id: form_id.into(),
            file,
        }
    }
}

/// What a pane asks the form to do besides its own commands: navigate or
/// add lines to the message log.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum PaneAction {
    #[default]
    None,
    /// `JumpTo`: make this record active (the navigation tree and the view
    /// follow it in steps 2 and 3).
    Jump(RecordSelection),
    /// Show the view tab (`mniRefByCompareSelectedClick` and
    /// `mniSpreadsheetCompareSelectedClick`).
    ShowView,
    /// Add these lines to the message log.
    Messages(Vec<String>),
    /// Put this text on the clipboard (`Clipboard.AsText`).
    Copy(String),
}

impl PaneAction {
    fn merge(self, other: PaneAction) -> PaneAction {
        match (self, other) {
            (PaneAction::None, other) => other,
            (this, PaneAction::None) => this,
            (PaneAction::Messages(mut left), PaneAction::Messages(right)) => {
                left.extend(right);
                PaneAction::Messages(left)
            }
            (PaneAction::Copy(left), PaneAction::Copy(right)) => PaneAction::Copy(format!(
                "{left}
{right}"
            )),
            (this, _) => this,
        }
    }
}

/// The lines of any command response that carries a `messages` array (the
/// compare and save commands); the GUI logs them as `PostAddMessage` does.
pub fn response_messages(value: &Value) -> Vec<String> {
    value
        .get("messages")
        .and_then(Value::as_array)
        .map(|messages| messages.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------
// The Referenced By tab
// ---------------------------------------------------------------------

/// One entry of the Referenced By list: upstream `TRefByListItem`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferencedByItem {
    /// `Name`: the record as xEdit displays it.
    pub name: String,
    pub signature: String,
    /// `FileName`: the plugin's name.
    pub file: String,
    /// `LoadOrderFormID`.
    pub form_id: String,
    /// `RawFileName`: the plugin's file name on disk.
    pub raw_file_name: String,
}

impl From<RecordRef> for ReferencedByItem {
    fn from(record: RecordRef) -> Self {
        Self {
            name: record_reference_name(&record),
            signature: record.signature,
            file: record.file.clone(),
            form_id: record.form_id,
            // The port's `RecordRef` names the plugin once; `_File.FileName`
            // and `_File.Name` are the same file for every plugin the loader
            // accepts, so the raw name follows the plugin name.
            raw_file_name: record.file,
        }
    }
}

/// `TRefByListItem.Name`: the record's `MainRecord.Name`
/// (`EditorID "Full Name" [SIG:FormID]`), with the editor ID as the
/// fallback for a record that has none.
fn record_reference_name(record: &RecordRef) -> String {
    if !record.name.is_empty() {
        record.name.clone()
    } else if record.editor_id.is_empty() {
        format!("[{}:{}]", record.signature, record.form_id)
    } else {
        record.editor_id.clone()
    }
}

/// `fpnlReferencedByFilter`: the three boxes and the And/Or switch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferencedByFilter {
    pub name: String,
    pub signature: String,
    pub file: String,
    /// `cobReferencedByFilter.ItemIndex = 1`.
    pub or: bool,
}

impl ReferencedByFilter {
    /// Whether any box holds filter text (`UseName or UseSig or UseFile`).
    pub fn is_active(&self) -> bool {
        !self.name.trim().is_empty() || !self.signature.trim().is_empty() || !self.file.trim().is_empty()
    }

    /// Port of `ApplyReferencedByFilter`'s predicate: name and signature
    /// joined by AND or OR, the file always AND. It is the same rule
    /// `refs.get` applies, and it is what the pane checks its rows against.
    pub fn matches(&self, item: &ReferencedByItem) -> bool {
        let name = self.name.trim().to_lowercase();
        let signature = self.signature.trim().to_lowercase();
        let file = self.file.trim().to_lowercase();
        let use_name = !name.is_empty();
        let use_signature = !signature.is_empty();
        let use_file = !file.is_empty();
        if !(use_name || use_signature || use_file) {
            return true;
        }
        let name_matches = !use_name || item.name.to_lowercase().contains(&name);
        let signature_matches = !use_signature || item.signature.to_lowercase().contains(&signature);
        let file_matches = !use_file || item.file.to_lowercase().contains(&file);
        file_matches
            && if self.or {
                name_matches || signature_matches
            } else {
                name_matches && signature_matches
            }
    }
}

/// The sort of `ApplyReferencedByAlphaSort`: the column (0 name, 1
/// signature, 2 file, 3 FormID, 4 raw file name) and the direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReferencedBySort {
    pub column: usize,
    pub reversed: bool,
}

impl ReferencedBySort {
    fn key<'a>(&self, item: &'a ReferencedByItem) -> &'a str {
        match self.column {
            1 => &item.signature,
            2 => &item.file,
            3 => &item.form_id,
            4 => &item.raw_file_name,
            _ => &item.name,
        }
    }
}

/// The `Record`, `Signature` and `File` columns; `FormID` and
/// `RawFileName` are the hidden columns 3 and 4 of the DFM.
const REFERENCED_BY_COLUMNS: [(&str, f32); 3] = [("Record", 200.0), ("Signature", 70.0), ("File", 200.0)];

/// The Referenced By tab (`lvReferencedBy` with `lvReferencedByAllItems`
/// and `lvReferencedByFilteredItems` collapsed into one list, because
/// `refs.get` applies the filter).
pub struct ReferencedByPane {
    pub filter: ReferencedByFilter,
    pub sort: ReferencedBySort,
    /// `lvReferencedByFilteredItems`.
    pub items: Vec<ReferencedByItem>,
    /// `ActiveMaster.ReferencedByCount`, the `N` of the caption.
    pub total: usize,
    /// `lvReferencedBy.Tag`-driven visibility: the tab appears once the
    /// active master has references.
    pub loaded_for: Option<RecordSelection>,
    pub selected: BTreeSet<usize>,
    pub focus: Option<usize>,
    pub error: Option<String>,
    pub columns: Vec<GridColumn>,
    pub state: GridState,
    /// The filter changed and the rows must be read again.
    pub reload: bool,
    /// The `Copy as ...` dialog: the copy flags of the chosen item.
    pub copy: Option<CopyRequest>,
    pub script: Option<String>,
    pub confirm_remove: bool,
    /// The suffix `UpdateCaption` adds to the sorted column.
    pub caption_column: usize,
}

/// The copy variants of `pmuRefBy` (`mniRefByCopyIntoClick`'s flags and
/// `mniRefByCopyDisabledOverrideIntoClick`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyRequest {
    pub as_new: bool,
    pub deep: bool,
    pub overwrite: bool,
    pub disable: bool,
    pub to: String,
}

impl CopyRequest {
    fn for_item(id: &str) -> Option<Self> {
        let (as_new, deep, overwrite, disable) = match id {
            "mniRefByCopyOverrideInto" => (false, false, false, false),
            "mniRefByCopyOverrideIntoWithOverwriting" => (false, false, true, false),
            "mniRefByDeepCopyOverrideInto" => (false, true, false, false),
            "mniRefByDeepCopyOverrideIntoWithOverwriting" => (false, true, true, false),
            "mniRefByCopyAsNewInto" => (true, false, false, false),
            "mniRefByCopyDisabledOverrideInto" => (false, false, false, true),
            _ => return None,
        };
        Some(Self {
            as_new,
            deep,
            overwrite,
            disable,
            to: String::new(),
        })
    }
}

impl ReferencedByPane {
    pub fn new() -> Self {
        let columns = REFERENCED_BY_COLUMNS
            .iter()
            .map(|(title, width)| GridColumn::new(*title, *width))
            .collect();
        Self {
            filter: ReferencedByFilter::default(),
            sort: ReferencedBySort::default(),
            items: Vec::new(),
            total: 0,
            loaded_for: None,
            selected: BTreeSet::new(),
            focus: None,
            error: None,
            columns,
            state: GridState::new(REFERENCED_BY_COLUMNS.iter().map(|(_, width)| *width).collect()),
            reload: false,
            copy: None,
            script: None,
            confirm_remove: false,
            caption_column: 0,
        }
    }

    /// `tbsReferencedBy.TabVisible := wbLoaderDone and (Tag > 0)`.
    pub fn visible(&self) -> bool {
        self.total > 0
    }

    /// The `refs.get` request the filter strip makes. The command applies
    /// the filter (`ApplyReferencedByFilter`) and returns the matching rows
    /// with `ReferencedByCount` before the filter.
    pub fn request(&self, selection: &RecordSelection) -> RefsGetRequest {
        let text = |value: &str| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        };
        RefsGetRequest {
            form_id: selection.form_id.clone(),
            file: selection.file.clone(),
            offset: 0,
            limit: None,
            filter_name: text(&self.filter.name),
            filter_signature: text(&self.filter.signature),
            filter_file: text(&self.filter.file),
            filter_or: self.filter.or,
        }
    }

    /// `pgMainChange`/`DoSetActiveRecord`: reads the list of the record's
    /// master through `refs.get`.
    pub fn load(&mut self, bridge: &mut CommandBridge, selection: &RecordSelection) -> Result<(), CommandError> {
        let request = self.request(selection);
        match bridge.request::<_, RefsGetResponse>("refs.get", &request) {
            Ok(response) => {
                self.items = response.referenced_by.into_iter().map(ReferencedByItem::from).collect();
                self.total = response.referenced_by_count;
                self.sort_items();
                self.selected.clear();
                self.focus = None;
                self.error = None;
                self.reload = false;
                self.loaded_for = Some(selection.clone());
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.message.clone());
                self.items.clear();
                self.total = 0;
                self.reload = false;
                Err(error)
            }
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.total = 0;
        self.selected.clear();
        self.focus = None;
        self.loaded_for = None;
        self.reload = false;
    }

    /// `ApplyReferencedByAlphaSort`: `CompareText` over the sort column.
    pub fn sort_items(&mut self) {
        let sort = self.sort;
        self.items.sort_by(|left, right| {
            let order = compare_text(sort.key(left), sort.key(right));
            if sort.reversed { order.reverse() } else { order }
        });
    }

    /// `lvReferencedByColumnClick`: the visible column maps to its DFM
    /// column; clicking the sorted Record or File column again switches to
    /// the hidden FormID or RawFileName column. The caller passes the
    /// Control key as `descending`.
    pub fn on_column_click(&mut self, visible: usize, descending: bool) {
        let mut column = match visible {
            1 => 1,
            2 => 2,
            _ => 0,
        };
        if self.sort.column == column {
            column = match column {
                0 => 3,
                2 => 4,
                other => other,
            };
        }
        self.sort = ReferencedBySort {
            column,
            reversed: descending,
        };
        self.sort_items();
    }

    /// `UpdateCaption`: the suffix of the sorted column (` [Name]`,
    /// ` [Prefixed]`, ` [FormID]`, ` [Raw]`) with the direction glyph.
    pub fn column_caption(&self, visible: usize) -> String {
        let title = REFERENCED_BY_COLUMNS
            .get(visible)
            .map(|(title, _)| *title)
            .unwrap_or("");
        let (caption_column, suffix) = match self.sort.column {
            0 => (0, " [Name]"),
            1 => (1, ""),
            2 => (2, " [Prefixed]"),
            3 => (0, " [FormID]"),
            4 => (2, " [Raw]"),
            _ => (0, ""),
        };
        if visible != caption_column {
            return title.to_owned();
        }
        let arrow = if self.sort.reversed { '\u{25BC}' } else { '\u{25B2}' };
        format!("{title}{suffix} {arrow}")
    }

    /// `UpdateReferencedByTabCaption`.
    pub fn caption(&self, active: bool) -> String {
        if active {
            format!(
                "Referenced by ({} / F: {} / S: {})",
                self.total,
                self.items.len(),
                self.selected.len()
            )
        } else {
            format!("Referenced By ({})", self.total)
        }
    }

    pub fn selected_items(&self) -> Vec<&ReferencedByItem> {
        self.selected
            .iter()
            .filter_map(|index| self.items.get(*index))
            .collect()
    }

    /// `lvReferencedByKeyDown`: Delete removes, Ctrl+A selects all, Ctrl+C
    /// copies the FormIDs (Shift+Alt copies the names).
    pub fn on_key(&mut self, key: egui::Key, modifiers: egui::Modifiers) -> Option<PaneAction> {
        match key {
            egui::Key::Delete if !self.selected.is_empty() => {
                self.confirm_remove = true;
                Some(PaneAction::None)
            }
            egui::Key::A if modifiers.ctrl => {
                self.selected = (0..self.items.len()).collect();
                Some(PaneAction::None)
            }
            egui::Key::C if modifiers.ctrl => {
                let names = modifiers.shift && modifiers.alt;
                let text: Vec<String> = self
                    .selected_items()
                    .iter()
                    .map(|item| if names { item.name.clone() } else { item.form_id.clone() })
                    .collect();
                if text.is_empty() {
                    None
                } else {
                    Some(PaneAction::Messages(vec![text.join("\r\n")]))
                }
            }
            _ => None,
        }
    }

    /// The `pmuRefBy` item that was clicked, as its effect.
    fn run_menu_item(&mut self, id: &str, bridge: &mut CommandBridge, edit_allowed: bool) -> PaneAction {
        let selected: Vec<ReferencedByItem> = self.selected_items().into_iter().cloned().collect();
        let mut messages = Vec::new();
        let mut action = PaneAction::None;
        match id {
            "mniRefByCompareSelected" => {
                if selected.len() >= 2 {
                    let records: Vec<Value> = selected
                        .iter()
                        .map(|item| json!({"form_id": item.form_id, "file": item.file}))
                        .collect();
                    match bridge.call("records.compare_selected", json!({"records": records})) {
                        Ok(response) => {
                            messages.extend(response_messages(&response));
                            action = PaneAction::ShowView;
                        }
                        Err(error) => messages.push(format!("Error: {}", error.message)),
                    }
                }
            }
            "mniRefByApplyScript" => {
                self.script = Some(String::new());
            }
            id if id.starts_with("mniRefByCopy") => {
                if let Some(mut copy) = CopyRequest::for_item(id) {
                    copy.to = self.selected_file().unwrap_or_default();
                    self.copy = Some(copy);
                }
            }
            "mniRefByRemove" => {
                if !selected.is_empty() && edit_allowed {
                    self.confirm_remove = true;
                }
            }
            "mniRefByMarkModified" => {
                if edit_allowed {
                    for item in &selected {
                        match bridge.call(
                            "elements.mark_modified",
                            json!({"form_id": item.form_id, "file": item.file}),
                        ) {
                            Ok(response) => messages.extend(response_messages(&response)),
                            Err(error) => messages.push(format!("Error: {}", error.message)),
                        }
                    }
                    self.reload = true;
                }
            }
            "mniRefByVWD" | "mniRefByNotVWD" if edit_allowed => {
                let visible = id == "mniRefByVWD";
                for item in selected.iter().filter(|item| item.signature == "REFR") {
                    match bridge.call(
                        "records.set_visible_when_distant",
                        json!({
                            "form_id": item.form_id,
                            "file": item.file,
                            "visible_when_distant": visible,
                        }),
                    ) {
                        Ok(response) => messages.extend(response_messages(&response)),
                        Err(error) => messages.push(format!("Error: {}", error.message)),
                    }
                }
                self.reload = true;
            }
            _ => {}
        }
        PaneAction::Messages(messages).merge(action)
    }

    fn selected_file(&self) -> Option<String> {
        self.selected_items().first().map(|item| item.file.clone())
    }

    /// The confirmed removal of `mniRefByRemoveClick`.
    fn remove_selected(&mut self, bridge: &mut CommandBridge) -> PaneAction {
        let selected: Vec<ReferencedByItem> = self.selected_items().into_iter().cloned().collect();
        let mut messages = Vec::new();
        for item in &selected {
            match bridge.call("records.delete", json!({"form_id": item.form_id, "file": item.file})) {
                Ok(response) => messages.extend(response_messages(&response)),
                Err(error) => messages.push(format!("Error: {}", error.message)),
            }
        }
        self.reload = true;
        PaneAction::Messages(messages)
    }

    fn run_copy(&mut self, bridge: &mut CommandBridge, copy: CopyRequest) -> PaneAction {
        let selected: Vec<ReferencedByItem> = self.selected_items().into_iter().cloned().collect();
        let mut messages = Vec::new();
        for item in &selected {
            match bridge.call(
                "records.copy",
                json!({
                    "form_id": item.form_id,
                    "from": item.file,
                    "to": copy.to,
                    "as_new": copy.as_new,
                    "deep": copy.deep,
                    "overwrite": copy.overwrite,
                    "disable": copy.disable,
                }),
            ) {
                Ok(response) => messages.extend(response_messages(&response)),
                Err(error) => messages.push(format!("Error: {}", error.message)),
            }
        }
        self.reload = true;
        PaneAction::Messages(messages)
    }

    fn run_script(&mut self, bridge: &mut CommandBridge, file: String) -> PaneAction {
        let mut messages = Vec::new();
        match bridge.call("script.run", json!({"file": file, "save": false, "build_refs": true})) {
            Ok(response) => messages.extend(response_messages(&response)),
            Err(error) => messages.push(format!("Error: {}", error.message)),
        }
        self.reload = true;
        PaneAction::Messages(messages)
    }
}

impl Default for ReferencedByPane {
    fn default() -> Self {
        Self::new()
    }
}

/// The grid view of a [`ReferencedByPane`]: the rows and the captions of
/// the sorted column, without the state the grid mutates.
struct ReferencedByGrid<'a> {
    items: &'a [ReferencedByItem],
    columns: Vec<GridColumn>,
}

impl GridSource for ReferencedByGrid<'_> {
    fn row_count(&self) -> usize {
        self.items.len()
    }

    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }

    fn cell(&self, row: usize, column: usize, out: &mut GridCell) {
        let Some(item) = self.items.get(row) else {
            return;
        };
        out.text = match column {
            0 => item.name.clone(),
            1 => item.signature.clone(),
            _ => item.file.clone(),
        };
    }
}

/// Draws the Referenced By tab: the filter strip, the list and its context
/// menu.
pub fn referenced_by_pane(
    ui: &mut egui::Ui,
    pane: &mut ReferencedByPane,
    bridge: &mut CommandBridge,
    selection: Option<&RecordSelection>,
    files: &[String],
    edit_allowed: bool,
    colors: &crate::widgets::conflict::ConflictColors,
) -> PaneAction {
    let mut action = PaneAction::None;
    if pane.reload
        && let Some(selection) = selection
        && let Err(_error) = pane.load(bridge, selection)
    {
        // The error is shown in the pane's own line.
    }
    // `fpnlReferencedByFilter`.
    ui.horizontal_wrapped(|ui| {
        ui.label("Filter by Record:");
        let mut changed = ui
            .add(egui::TextEdit::singleline(&mut pane.filter.name).desired_width(120.0))
            .changed();
        egui::ComboBox::from_id_salt("cobReferencedByFilter")
            .selected_text(if pane.filter.or { "or" } else { "and" })
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(&mut pane.filter.or, false, "and").changed();
                changed |= ui.selectable_value(&mut pane.filter.or, true, "or").changed();
            });
        ui.label("by Signature:");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut pane.filter.signature).desired_width(120.0))
            .changed();
        ui.label("AND by FileName:");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut pane.filter.file).desired_width(120.0))
            .changed();
        if changed {
            pane.reload = true;
        }
    });
    if let Some(error) = &pane.error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    let columns: Vec<GridColumn> = REFERENCED_BY_COLUMNS
        .iter()
        .enumerate()
        .map(|(index, (_, width))| GridColumn::new(pane.column_caption(index), *width))
        .collect();
    let mut source = ReferencedByGrid {
        items: &pane.items,
        columns,
    };
    let output = Grid {
        id: ui.scope_id().with("lvReferencedBy"),
        source: &mut source,
        state: &mut pane.state,
        colors,
        header: true,
        context_menu: None,
        own_double_click: true,
    }
    .show(ui);
    if let Some((column, _)) = output.header_clicked {
        // `lvReferencedByColumnClick`: Control sorts descending.
        let descending = ui.input(|input| input.modifiers.ctrl);
        pane.on_column_click(column, descending);
    }
    if let Some((row, _)) = output.clicked {
        pane.focus = Some(row);
        if ui.input(|input| input.modifiers.shift) {
            pane.selected.insert(row);
        } else {
            pane.selected.clear();
            pane.selected.insert(row);
        }
    }
    if let Some((row, _)) = output.double_clicked
        && let Some(item) = pane.items.get(row)
    {
        action = PaneAction::Jump(RecordSelection::new(&item.form_id, Some(item.file.clone())));
    }
    if let Some((row, _)) = output.secondary_clicked {
        pane.focus = Some(row);
        if !pane.selected.contains(&row) {
            pane.selected.clear();
            pane.selected.insert(row);
        }
    }
    if ui.memory(|memory| memory.has_focus(ui.scope_id().with("lvReferencedBy"))) {
        for key in [egui::Key::Delete, egui::Key::A, egui::Key::C] {
            if ui.input(|input| input.key_pressed(key)) {
                let modifiers = ui.input(|input| input.modifiers);
                if let Some(key_action) = pane.on_key(key, modifiers) {
                    action = action.merge(key_action);
                }
                ui.input_mut(|input| input.consume_key(modifiers, key));
            }
        }
    }
    let response = ui.response();
    let mut clicked_item: Option<&'static str> = None;
    crate::menu_model::container_context_menu(&response, |ui| {
        clicked_item = crate::menu_model::draw_menu(ui, &crate::menu_model::PMU_REF_BY, &|item| {
            crate::menu_model::MenuItemState {
                enabled: match item.id {
                    "mniRefByCompareSelected" => pane.selected.len() >= 2,
                    id if id.starts_with("mniRefByCopy") => edit_allowed && !pane.selected.is_empty(),
                    "mniRefByRemove" | "mniRefByMarkModified" | "mniRefByVWD" | "mniRefByNotVWD" => {
                        edit_allowed && !pane.selected.is_empty()
                    }
                    _ => item.enabled,
                },
                checked: item.checked,
            }
        })
        .map(|item| item.id);
    });
    if let Some(id) = clicked_item {
        action = action.merge(pane.run_menu_item(id, bridge, edit_allowed));
    }
    // The Copy into dialog (`TfrmFileSelect`, step 5 owns its full form).
    if let Some(copy) = pane.copy.clone() {
        let mut open = true;
        let mut run = false;
        egui::Window::new("Copy as override into....")
            .id(egui::Id::unique("refby-copy"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                egui::ComboBox::from_id_salt("refby-copy-target")
                    .selected_text(if copy.to.is_empty() {
                        "choose a plugin"
                    } else {
                        &copy.to
                    })
                    .show_ui(ui, |ui| {
                        for file in files {
                            ui.selectable_value(&mut pane.copy.as_mut().unwrap().to, file.clone(), file);
                        }
                    });
                run = ui.add_enabled(!copy.to.is_empty(), egui::Button::new("Copy")).clicked();
            });
        if run && let Some(copy) = pane.copy.take() {
            action = action.merge(pane.run_copy(bridge, copy));
        } else if !open {
            pane.copy = None;
        }
    }
    if pane.confirm_remove {
        let count = pane.selected.len();
        let mut open = true;
        let mut remove = false;
        egui::Window::new("Confirm")
            .id(egui::Id::unique("refby-remove"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label(format!("Permanently remove the {count} selected record(s)?"));
                remove = ui.button("Yes").clicked();
            });
        if remove {
            pane.confirm_remove = false;
            action = action.merge(pane.remove_selected(bridge));
        } else if !open {
            pane.confirm_remove = false;
        }
    }
    if let Some(file) = pane.script.clone() {
        let mut open = true;
        let mut run = false;
        let mut file = file;
        egui::Window::new("Apply Script")
            .id(egui::Id::unique("refby-script"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label("Script file:");
                ui.text_edit_singleline(&mut file);
                run = ui
                    .add_enabled(!file.trim().is_empty(), egui::Button::new("Run"))
                    .clicked();
            });
        if run {
            pane.script = None;
            action = action.merge(pane.run_script(bridge, file));
        } else if !open {
            pane.script = None;
        }
    }
    action
}

// ---------------------------------------------------------------------
// The Messages tab
// ---------------------------------------------------------------------

/// `mmoMessages`: the log lines and their presentation.
#[derive(Debug, Clone, PartialEq)]
pub struct MessagesPane {
    /// `mmoMessages.Lines`.
    pub lines: Vec<String>,
    /// `mniMessagesAutoscroll.Checked`.
    pub autoscroll: bool,
    /// `tbsMessages.Highlighted` while another tab is active.
    pub highlighted: bool,
    /// The selected text of the last frame (`mmoMessages.SelText`).
    pub selected: String,
    /// The Save dialog of `mniMessagesSaveSelectedClick`.
    pub save: Option<String>,
    pub error: Option<String>,
}

impl MessagesPane {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            autoscroll: true,
            highlighted: false,
            selected: String::new(),
            save: None,
            error: None,
        }
    }

    /// `AddMessage`: one line per CRLF part; `stbMain` takes the last.
    pub fn add(&mut self, text: &str) {
        for line in text.split("\r\n") {
            self.lines.push(line.to_owned());
        }
    }

    /// `mmoMessages.Lines.Add`: one line, without splitting it at CRLF.
    pub fn push(&mut self, line: String) {
        self.lines.push(line);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.selected.clear();
    }

    pub fn last(&self) -> String {
        self.lines.last().cloned().unwrap_or_default()
    }

    /// `mmoMessages.Lines.Text`.
    pub fn text(&self) -> String {
        self.lines.join("\r\n")
    }

    /// `mniMessagesSaveSelectedClick`: the selected text, trimmed, written
    /// to the chosen file; nothing when nothing is selected.
    pub fn save_selected(&mut self) {
        let text = self.selected.trim();
        if text.is_empty() {
            return;
        }
        let Some(mut path) = self.save.take() else {
            return;
        };
        if std::path::Path::new(&path).extension().is_none() {
            path.push_str(".txt");
        }
        match std::fs::write(&path, text) {
            Ok(()) => self.error = None,
            Err(error) => self.error = Some(format!("Could not write {path}: {error}")),
        }
    }

    /// Draws the memo and its context menu. `id` identifies the read-only
    /// editor so the selection can be read back (`Memo1`/`mmoMessages` are
    /// `TMemo`s with `HideSelection = False`).
    pub fn show(&mut self, ui: &mut egui::Ui, font: egui::FontId, id: egui::Id) -> PaneAction {
        let mut action = PaneAction::None;
        let text = self.text();
        let mut editable = text.as_str();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .stick_to_bottom(self.autoscroll)
            .show(ui, |ui| {
                let output = egui::TextEdit::multiline(&mut editable)
                    .id(id)
                    .font(font)
                    .desired_width(f32::INFINITY)
                    .lock_focus(true)
                    .show(ui);
                self.selected = selected_text(ui.ctx(), id, &text);
                if output.response.double_clicked()
                    && ui.input(|input| input.modifiers.ctrl)
                    && let Some(form_id) = parse_message_form_id(&self.selected)
                {
                    action = PaneAction::Jump(RecordSelection::new(form_id, None));
                }
            });
        let response = ui.response();
        let mut clicked: Option<&'static str> = None;
        crate::menu_model::container_context_menu(&response, |ui| {
            clicked = crate::menu_model::draw_menu(ui, &crate::menu_model::PMU_MESSAGES, &|item| {
                crate::menu_model::MenuItemState {
                    enabled: match item.id {
                        "mniMessagesSaveSelected" => !self.selected.trim().is_empty(),
                        _ => item.enabled,
                    },
                    checked: match item.id {
                        "mniMessagesAutoscroll" => self.autoscroll,
                        _ => item.checked,
                    },
                }
            })
            .map(|item| item.id);
        });
        if let Some(item) = clicked {
            match item {
                "mniMessagesClear" => self.clear(),
                "mniMessagesSaveSelected" => {
                    if !self.selected.trim().is_empty() && self.save.is_none() {
                        let folder = xedit_core::interface::globals::program_path();
                        self.save = Some(format!("{folder}Messages.txt"));
                    }
                }
                "mniMessagesAutoscroll" => self.autoscroll = !self.autoscroll,
                _ => {}
            }
        }
        if let Some(path) = self.save.clone() {
            let mut open = true;
            let mut save = false;
            let mut path = path;
            egui::Window::new("Save selected text")
                .id(egui::Id::unique("messages-save"))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.text_edit_singleline(&mut path);
                    save = ui.button("Save").clicked();
                });
            if save {
                self.save = Some(path);
                self.save_selected();
            } else if !open {
                self.save = None;
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        action
    }
}

impl Default for MessagesPane {
    fn default() -> Self {
        Self::new()
    }
}

/// The text selected in a read-only `TextEdit`, from its cursor state. The
/// indices are character indices, as egui stores them.
pub fn selected_text(ctx: &egui::Context, id: egui::Id, text: &str) -> String {
    let Some(state) = egui::text_edit::TextEditState::load(ctx, id) else {
        return String::new();
    };
    let Some(range) = state.cursor.char_range() else {
        return String::new();
    };
    let start = range.primary.index.0.min(range.secondary.index.0);
    let end = range.primary.index.0.max(range.secondary.index.0);
    if start == end {
        return String::new();
    }
    text.chars().skip(start).take(end - start).collect()
}

/// `mmoMessagesDblClick`: the FormID of the line the user selected, when
/// the selection is exactly one (optionally bracketed) eight-digit id.
pub fn parse_message_form_id(text: &str) -> Option<String> {
    let mut text = text.trim().to_owned();
    if text.len() < 8 {
        return None;
    }
    if let Some(index) = text.find('[')
        && index > 0
    {
        text.drain(..index);
    }
    if let Some(index) = text.find(']') {
        text.truncate(index + 1);
    }
    if text.starts_with('[') && text.ends_with(']') {
        text = text[1..text.len() - 1].to_owned();
    }
    if text.len() < 8 {
        return None;
    }
    // `if s[5] = ':'` -- the `0000:0000` form.
    if text.as_bytes()[4] == b':' {
        text = text[5..].to_owned();
    }
    if text.len() != 8 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(text.to_ascii_uppercase())
}

// ---------------------------------------------------------------------
// The Information tab
// ---------------------------------------------------------------------

/// `Memo1.Lines.Strings` of `xeMainForm.dfm`, in order. `Memo1` is
/// `ReadOnly`, `WordWrap = False`, `ScrollBars = ssVertical` and is set to
/// the monospace font of the form (`Consolas`, else `Courier New`). 4.1.5q
/// never changes the lines after the form loads: the tab has static content
/// and only the auto modes hide it (`tbsInfo.TabVisible := False`).
pub const INFORMATION_TEXT: &str = include_str!("information_text.txt");

/// Draws `tbsInfo`: the read-only memo.
pub fn information_pane(ui: &mut egui::Ui, font: egui::FontId, id: egui::Id) -> String {
    let mut read_only = INFORMATION_TEXT;
    let mut selected = String::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::TextEdit::multiline(&mut read_only)
            .id(id)
            .font(font)
            .desired_width(f32::INFINITY)
            .lock_focus(true)
            .show(ui);
        selected = selected_text(ui.ctx(), id, INFORMATION_TEXT);
    });
    selected
}

// ---------------------------------------------------------------------
// The localization view of the selected record
// ---------------------------------------------------------------------

/// One localized string of the selected record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalizationRow {
    /// The element's path in the record (`FULL`, `DESC`, `Responses\...`).
    pub path: String,
    pub table: String,
    pub id: String,
    pub text: String,
}

/// The read-only localization view: for the selected record, the strings
/// `mniViewEditClick` would open `TfrmLocalization` on (a translatable
/// string of a localized plugin), resolved through `localization.get`.
#[derive(Default)]
pub struct LocalizationPane {
    pub rows: Vec<LocalizationRow>,
    pub loaded_for: Option<RecordSelection>,
    pub error: Option<String>,
    pub visible: bool,
}

impl LocalizationPane {
    /// Reads the record and asks `localization.get` about its leaf elements.
    /// A plugin that is not localized answers `not_localized`, and a value
    /// that is not a string answers `unknown_element` or `not_localized`;
    /// both simply do not appear in the view.
    pub fn load(&mut self, bridge: &mut CommandBridge, selection: &RecordSelection) -> Result<(), CommandError> {
        self.rows.clear();
        self.loaded_for = Some(selection.clone());
        self.visible = false;
        // `mniViewEditClick` opens the localization editor only for a
        // string of a localized plugin: a plugin without tables has none.
        let files: xedit_session::localization::LocalizationFilesResponse = bridge.request(
            "localization.files",
            &xedit_session::localization::LocalizationFilesRequest {
                file: selection.file.clone(),
                load: true,
            },
        )?;
        if files.tables.is_empty() {
            return Ok(());
        }
        let request = RecordsGetRequest {
            form_id: selection.form_id.clone(),
            file: selection.file.clone(),
            depth: None,
        };
        let detail: RecordDetail = bridge.request("records.get", &request)?;
        let mut candidates = Vec::new();
        collect_leaf_paths(&detail.elements, "", &mut candidates);
        // Localized strings are few; the cap keeps a record with thousands
        // of values from turning the view into thousands of commands.
        candidates.truncate(256);
        for path in candidates {
            let value = bridge.call(
                "localization.get",
                json!({
                    "form_id": selection.form_id,
                    "path": path,
                    "file": selection.file,
                }),
            );
            let Ok(value) = value else {
                continue;
            };
            let Ok(found) = serde_json::from_value::<xedit_session::localization::LocalizationGetResponse>(value)
            else {
                continue;
            };
            if !found.found || found.text.is_empty() {
                continue;
            }
            self.rows.push(LocalizationRow {
                path,
                table: found.table,
                id: found.id,
                text: found.text,
            });
        }
        self.visible = !self.rows.is_empty();
        Ok(())
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.loaded_for = None;
        self.visible = false;
    }

    pub fn show(&self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("Localization");
        if self.rows.is_empty() {
            ui.label("The selected record has no localized strings.");
            return;
        }
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .max_height(160.0)
            .show(ui, |ui| {
                egui::Grid::new("localization-rows").striped(true).show(ui, |ui| {
                    ui.label("Element");
                    ui.label("Table");
                    ui.label("ID");
                    ui.label("Text");
                    ui.end_row();
                    for row in &self.rows {
                        ui.label(&row.path);
                        ui.label(&row.table);
                        ui.label(&row.id);
                        ui.label(&row.text);
                        ui.end_row();
                    }
                });
            });
    }
}

fn collect_leaf_paths(elements: &[xedit_session::commands::ElementNode], prefix: &str, out: &mut Vec<String>) {
    for element in elements {
        let path = if prefix.is_empty() {
            element.name.clone()
        } else {
            format!("{prefix}\\{}", element.name)
        };
        if element.children.is_empty() {
            out.push(path);
        } else {
            collect_leaf_paths(&element.children, &path, out);
        }
    }
}

// ---------------------------------------------------------------------
// The three spreadsheets
// ---------------------------------------------------------------------

/// `tbsWEAPSpreadsheet`, `tbsARMOSpreadsheet`, `tbsAMMOSpreadsheet`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpreadsheetKind {
    Weapon,
    Armor,
    Ammo,
}

impl SpreadsheetKind {
    pub const ALL: [SpreadsheetKind; 3] = [SpreadsheetKind::Weapon, SpreadsheetKind::Armor, SpreadsheetKind::Ammo];

    /// The group signature the tab lists (`StrToSignature(Copy(Name, 4, 4))`).
    pub fn signature(self) -> &'static str {
        match self {
            SpreadsheetKind::Weapon => "WEAP",
            SpreadsheetKind::Armor => "ARMO",
            SpreadsheetKind::Ammo => "AMMO",
        }
    }

    /// The settings section (`Signature + ' Spreadsheet'`).
    pub fn settings_section(self) -> &'static str {
        match self {
            SpreadsheetKind::Weapon => "WEAP Spreadsheet",
            SpreadsheetKind::Armor => "ARMO Spreadsheet",
            SpreadsheetKind::Ammo => "AMMO Spreadsheet",
        }
    }

    fn columns(self) -> &'static [ColumnSpec] {
        match self {
            SpreadsheetKind::Weapon => &WEAPON_COLUMNS,
            SpreadsheetKind::Armor => &ARMOR_COLUMNS,
            SpreadsheetKind::Ammo => &AMMO_COLUMNS,
        }
    }
}

/// One `Columns` item of the DFM. `decl` is the index the code uses
/// (`Header.Columns[i]`), which is the order the items are declared in, and
/// `position` is the display order of the `Position` property.
#[derive(Clone, Copy, Debug)]
pub struct ColumnSpec {
    pub decl: usize,
    pub position: usize,
    pub title: &'static str,
    pub width: f32,
    pub right: bool,
    /// `coVisible` absent.
    pub hidden: bool,
}

const WEAPON_COLUMNS: [ColumnSpec; 21] = [
    ColumnSpec {
        decl: 0,
        position: 0,
        title: "File Name",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 1,
        position: 1,
        title: "FormID",
        width: 75.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 2,
        position: 2,
        title: "EditorID",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 3,
        position: 3,
        title: "Weapon Name",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 4,
        position: 4,
        title: "Enchantment",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 5,
        position: 6,
        title: "Type",
        width: 120.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 6,
        position: 8,
        title: "Speed",
        width: 85.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 7,
        position: 9,
        title: "Reach",
        width: 85.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 8,
        position: 10,
        title: "Value",
        width: 65.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 9,
        position: 11,
        title: "Health",
        width: 65.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 10,
        position: 12,
        title: "Weight",
        width: 85.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 11,
        position: 13,
        title: "Damage",
        width: 65.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 12,
        position: 5,
        title: "Amount",
        width: 70.0,
        right: true,
        hidden: true,
    },
    ColumnSpec {
        decl: 13,
        position: 7,
        title: "Skill",
        width: 65.0,
        right: false,
        hidden: true,
    },
    ColumnSpec {
        decl: 14,
        position: 14,
        title: "Stagger",
        width: 65.0,
        right: true,
        hidden: true,
    },
    ColumnSpec {
        decl: 15,
        position: 15,
        title: "Crit. Damage",
        width: 65.0,
        right: true,
        hidden: true,
    },
    ColumnSpec {
        decl: 16,
        position: 16,
        title: "Crit. % Mult.",
        width: 65.0,
        right: true,
        hidden: true,
    },
    ColumnSpec {
        decl: 17,
        position: 17,
        title: "Range Min",
        width: 65.0,
        right: true,
        hidden: true,
    },
    ColumnSpec {
        decl: 18,
        position: 18,
        title: "Range Max",
        width: 65.0,
        right: true,
        hidden: true,
    },
    ColumnSpec {
        decl: 19,
        position: 19,
        title: "Sound",
        width: 65.0,
        right: false,
        hidden: true,
    },
    ColumnSpec {
        decl: 20,
        position: 20,
        title: "Template",
        width: 120.0,
        right: false,
        hidden: true,
    },
];

const ARMOR_COLUMNS: [ColumnSpec; 13] = [
    ColumnSpec {
        decl: 0,
        position: 0,
        title: "File Name",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 1,
        position: 1,
        title: "FormID",
        width: 75.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 2,
        position: 2,
        title: "EditorID",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 3,
        position: 3,
        title: "Armor Name",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 4,
        position: 4,
        title: "Enchantment",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 5,
        position: 5,
        title: "Slots",
        width: 120.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 6,
        position: 6,
        title: "Type",
        width: 110.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 7,
        position: 8,
        title: "Armor",
        width: 85.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 8,
        position: 9,
        title: "Value",
        width: 65.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 9,
        position: 10,
        title: "Health",
        width: 65.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 10,
        position: 11,
        title: "Weight",
        width: 85.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 11,
        position: 7,
        title: "Equip. Type",
        width: 115.0,
        right: false,
        hidden: true,
    },
    ColumnSpec {
        decl: 12,
        position: 12,
        title: "Template",
        width: 110.0,
        right: false,
        hidden: true,
    },
];

const AMMO_COLUMNS: [ColumnSpec; 10] = [
    ColumnSpec {
        decl: 0,
        position: 0,
        title: "File Name",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 1,
        position: 1,
        title: "FormID",
        width: 75.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 2,
        position: 2,
        title: "EditorID",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 3,
        position: 3,
        title: "Ammunition Name",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 4,
        position: 4,
        title: "Enchantment",
        width: 150.0,
        right: false,
        hidden: false,
    },
    ColumnSpec {
        decl: 5,
        position: 5,
        title: "Speed",
        width: 85.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 6,
        position: 6,
        title: "Value",
        width: 65.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 7,
        position: 7,
        title: "Weight",
        width: 85.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 8,
        position: 8,
        title: "Damage",
        width: 65.0,
        right: true,
        hidden: false,
    },
    ColumnSpec {
        decl: 9,
        position: 9,
        title: "Flags",
        width: 80.0,
        right: false,
        hidden: true,
    },
];

/// Where the value of a spreadsheet column comes from
/// (`vstSpreadSheet*InitNode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellSource {
    FileName,
    FormId,
    /// `RecordBySignature['X']`.
    Top(&'static str),
    /// `RecordBySignature['X'].Elements[i]`.
    DataIndex(&'static str, usize),
    /// `RecordBySignature['X'].ElementByName['Name']`.
    DataNamed(&'static str, &'static str),
    /// The column is not filled for the game (upstream leaves the element
    /// unset).
    None,
}

/// The element a column shows: the `Table` (the columns of the DFM) of
/// `vstSpreadSheet*InitNode` for Oblivion and for Skyrim.
pub fn cell_source(kind: SpreadsheetKind, decl: usize, skyrim: bool) -> CellSource {
    match (kind, decl) {
        (_, 0) => CellSource::FileName,
        (_, 1) => CellSource::FormId,
        (_, 2) => CellSource::Top("EDID"),
        (_, 3) => CellSource::Top("FULL"),
        (SpreadsheetKind::Weapon, 4) => CellSource::Top(if skyrim { "EITM" } else { "ENAM" }),
        (SpreadsheetKind::Armor, 4) => CellSource::Top(if skyrim { "EITM" } else { "ENAM" }),
        (SpreadsheetKind::Ammo, 4) => {
            if skyrim {
                CellSource::DataNamed("DATA", "Projectile")
            } else {
                CellSource::Top("ENAM")
            }
        }
        (SpreadsheetKind::Weapon, 5) => {
            if skyrim {
                CellSource::Top("ETYP")
            } else {
                CellSource::DataIndex("DATA", 0)
            }
        }
        (SpreadsheetKind::Armor, 5) => {
            if skyrim {
                CellSource::DataIndex("BODT", 0)
            } else {
                CellSource::DataIndex("BMDT", 0)
            }
        }
        (SpreadsheetKind::Armor, 6) => {
            if skyrim {
                CellSource::DataIndex("BODT", 3)
            } else {
                CellSource::DataIndex("BMDT", 1)
            }
        }
        (SpreadsheetKind::Weapon, _) if skyrim => match decl {
            6 => CellSource::DataIndex("DNAM", 2),
            7 => CellSource::DataIndex("DNAM", 3),
            8 => CellSource::DataIndex("DATA", 0),
            10 => CellSource::DataIndex("DATA", 1),
            11 => CellSource::DataIndex("DATA", 2),
            12 => CellSource::Top("EAMT"),
            13 => CellSource::DataNamed("DNAM", "Skill"),
            14 => CellSource::DataNamed("DNAM", "Stagger"),
            15 => CellSource::DataNamed("CRDT", "Damage"),
            16 => CellSource::DataNamed("CRDT", "% Mult"),
            17 => CellSource::DataNamed("DNAM", "Range Min"),
            18 => CellSource::DataNamed("DNAM", "Range Max"),
            19 => CellSource::Top("VNAM"),
            20 => CellSource::Top("CNAM"),
            _ => CellSource::None,
        },
        (SpreadsheetKind::Weapon, _) => match decl {
            6 => CellSource::DataIndex("DATA", 1),
            7 => CellSource::DataIndex("DATA", 2),
            8 => CellSource::DataIndex("DATA", 4),
            9 => CellSource::DataIndex("DATA", 5),
            10 => CellSource::DataIndex("DATA", 6),
            11 => CellSource::DataIndex("DATA", 7),
            _ => CellSource::None,
        },
        (SpreadsheetKind::Armor, _) if skyrim => match decl {
            7 => CellSource::Top("DNAM"),
            8 => CellSource::DataIndex("DATA", 0),
            9 => CellSource::None,
            10 => CellSource::DataIndex("DATA", 1),
            11 => CellSource::Top("ETYP"),
            12 => CellSource::Top("TNAM"),
            _ => CellSource::None,
        },
        (SpreadsheetKind::Armor, _) => match decl {
            7 => CellSource::DataIndex("DATA", 0),
            8 => CellSource::DataIndex("DATA", 1),
            9 => CellSource::DataIndex("DATA", 2),
            10 => CellSource::DataIndex("DATA", 3),
            _ => CellSource::None,
        },
        (SpreadsheetKind::Ammo, _) if skyrim => match decl {
            5 | 7 => CellSource::None,
            6 => CellSource::DataNamed("DATA", "Value"),
            8 => CellSource::DataNamed("DATA", "Damage"),
            9 => CellSource::DataNamed("DATA", "Flags"),
            _ => CellSource::None,
        },
        (SpreadsheetKind::Ammo, _) => match decl {
            5 => CellSource::DataNamed("DATA", "Speed"),
            6 => CellSource::DataNamed("DATA", "Value"),
            7 => CellSource::DataNamed("DATA", "Weight"),
            8 => CellSource::DataNamed("DATA", "Damage"),
            _ => CellSource::None,
        },
    }
}

/// The visible columns of a spreadsheet, sorted by `Position`, with the
/// Skyrim adjustments of `WMUserLoaderDone` applied.
pub fn spreadsheet_columns(kind: SpreadsheetKind, skyrim: bool) -> Vec<(ColumnSpec, String)> {
    let mut columns: Vec<(ColumnSpec, String)> = kind
        .columns()
        .iter()
        .filter_map(|spec| {
            let mut spec = *spec;
            if skyrim {
                // `if wbIsSkyrim then`: hide the fields Skyrim does not have
                // and show the ones its records carry.
                match (kind, spec.decl) {
                    (SpreadsheetKind::Weapon, 9) => spec.hidden = true,
                    (SpreadsheetKind::Weapon, 12..=20) => spec.hidden = false,
                    (SpreadsheetKind::Armor, 9) => spec.hidden = true,
                    (SpreadsheetKind::Armor, 11..=12) => spec.hidden = false,
                    (SpreadsheetKind::Ammo, 5 | 7) => spec.hidden = true,
                    (SpreadsheetKind::Ammo, 9) => spec.hidden = false,
                    _ => {}
                }
            }
            if spec.hidden {
                return None;
            }
            let mut title = spec.title.to_owned();
            if skyrim {
                match (kind, spec.decl) {
                    (SpreadsheetKind::Armor, 6) => title = "Armor Type".to_owned(),
                    (SpreadsheetKind::Ammo, 4) => title = "Projectile".to_owned(),
                    _ => {}
                }
            }
            Some((spec, title))
        })
        .collect();
    columns.sort_by_key(|(spec, _)| spec.position);
    columns
}

/// One row of a spreadsheet: the record and its element values, read once
/// when the tab is shown (`vstSpreadSheet*InitNode`).
#[derive(Clone, Debug)]
pub struct SpreadsheetRow {
    pub record: RecordSummary,
    pub detail: RecordDetail,
}

/// `PSpreadsheetNodeDatas`: the element values of one row, found by walking
/// the `RecordDetail`.
impl SpreadsheetRow {
    pub fn element(&self, source: CellSource) -> Option<&xedit_session::commands::ElementNode> {
        match source {
            CellSource::FileName | CellSource::FormId | CellSource::None => None,
            CellSource::Top(signature) => self.detail.elements.iter().find(|node| node_named(node, signature)),
            CellSource::DataIndex(signature, index) => self
                .detail
                .elements
                .iter()
                .find(|node| node_named(node, signature))
                // `vstSpreadsheetArmorInitNode`: BOD2 when BODT is absent.
                .or_else(|| {
                    (signature == "BODT")
                        .then(|| self.detail.elements.iter().find(|node| node_named(node, "BOD2")))
                        .flatten()
                })
                .and_then(|node| node.children.get(index)),
            CellSource::DataNamed(signature, name) => self
                .detail
                .elements
                .iter()
                .find(|node| node_named(node, signature))
                .and_then(|node| node.children.iter().find(|node| node_named(node, name))),
        }
    }

    /// The path `elements.set` takes to the element a column shows: the
    /// signature of the subrecord and the name or the position of the
    /// member below it, the way `ElementByPath` resolves it.
    pub fn path(&self, source: CellSource) -> Option<String> {
        // The element has to be there: a column without one has no path.
        self.element(source)?;
        match source {
            CellSource::FileName | CellSource::FormId | CellSource::None => None,
            CellSource::Top(signature) => Some(signature.to_owned()),
            CellSource::DataIndex(signature, index) => Some(format!("{}\\[{index}]", self.owner_signature(signature))),
            CellSource::DataNamed(signature, name) => Some(format!("{}\\{name}", self.owner_signature(signature))),
        }
    }

    /// The subrecord a column reads: the one it names, or `BOD2` when it
    /// names `BODT` and the record has no `BODT` (`vstSpreadsheetArmorInitNode`).
    fn owner_signature(&self, signature: &'static str) -> &'static str {
        if signature == "BODT"
            && !self.detail.elements.iter().any(|node| node_named(node, "BODT"))
            && self.detail.elements.iter().any(|node| node_named(node, "BOD2"))
        {
            return "BOD2";
        }
        signature
    }

    /// `vstSpreadSheetGetText`.
    pub fn text(&self, kind: SpreadsheetKind, decl: usize, skyrim: bool) -> String {
        match decl {
            0 => self.record.file.clone(),
            1 => pretty_form_id(&self.record.form_id),
            _ => match self.element(cell_source(kind, decl, skyrim)) {
                Some(element) => element_text(element),
                None => String::new(),
            },
        }
    }
}

/// Whether an element is the one a column names: the definition name, the
/// display name, or the signature a top-level subrecord's display name
/// starts with (`EDID - Editor ID` is `RecordBySignature['EDID']`).
fn node_named(node: &xedit_session::commands::ElementNode, name: &str) -> bool {
    if node.name.eq_ignore_ascii_case(name)
        || node
            .display_name
            .as_deref()
            .is_some_and(|display| display.eq_ignore_ascii_case(name))
    {
        return true;
    }
    if name.len() == 4
        && let Some((signature, _)) = node.name.split_once(" - ")
    {
        return signature.eq_ignore_ascii_case(name);
    }
    false
}

/// `vstSpreadSheetGetText` for a linked value: the editor ID of the record
/// it links to, else its load order FormID. The command's node carries the
/// display text `EditorID [SIG:FormID]`, so the editor ID is the text
/// before the bracket.
pub fn element_text(element: &xedit_session::commands::ElementNode) -> String {
    let value = element.value.clone();
    if let Some((head, tail)) = value.rsplit_once('[')
        && tail.ends_with(']')
    {
        let head = head.trim();
        if !head.is_empty() {
            return head.to_owned();
        }
        if let Some((_, id)) = tail.trim_end_matches(']').split_once(':') {
            return pretty_form_id(id);
        }
    }
    value
}

/// `%08X` with the pretty form of `wbDisplayLoadOrderFormID` (a space after
/// the file index, and another for light or medium slots).
pub fn pretty_form_id(form_id: &str) -> String {
    let digits: String = form_id.chars().filter(|c| c.is_ascii_hexdigit()).take(8).collect();
    if digits.len() != 8 {
        return digits;
    }
    let value = u32::from_str_radix(&digits, 16).unwrap_or(0);
    xedit_core::interface::form_id::FormID::from_cardinal(value).to_string(true)
}

/// `vstSpreadSheetShow`: one spreadsheet tab.
pub struct SpreadsheetPane {
    pub kind: SpreadsheetKind,
    pub rows: Vec<SpreadsheetRow>,
    /// The files the tab lists; empty means every loaded file with the
    /// group, as the file selection dialog starts.
    pub files: Vec<String>,
    pub loaded: bool,
    pub error: Option<String>,
    pub selected: BTreeSet<usize>,
    pub focus: Option<usize>,
    pub columns: Vec<GridColumn>,
    /// The DFM declaration index of each visible column, in display order.
    pub decls: Vec<usize>,
    /// The columns and cells are the Skyrim table (`wbIsSkyrim`).
    pub skyrim: bool,
    pub state: GridState,
}

impl SpreadsheetPane {
    pub fn new(kind: SpreadsheetKind) -> Self {
        Self {
            kind,
            rows: Vec::new(),
            files: Vec::new(),
            loaded: false,
            error: None,
            selected: BTreeSet::new(),
            focus: None,
            columns: Vec::new(),
            decls: Vec::new(),
            skyrim: false,
            state: GridState::new(Vec::new()),
        }
    }

    /// `tbsSpreadsheetShow`: asks for the records of the group in every
    /// selected file and reads each one.
    pub fn load(&mut self, bridge: &mut CommandBridge, files: &[String], skyrim: bool) -> Result<(), CommandError> {
        self.rows.clear();
        self.columns.clear();
        self.decls.clear();
        self.skyrim = skyrim;
        for (spec, title) in spreadsheet_columns(self.kind, skyrim) {
            let mut column = GridColumn::new(title, spec.width);
            column.right_aligned = spec.right;
            column.min_width = spec.width;
            self.columns.push(column);
            self.decls.push(spec.decl);
        }
        let selected: Vec<String> = if self.files.is_empty() {
            files.to_vec()
        } else {
            self.files.clone()
        };
        self.state = GridState::new(self.columns.iter().map(|column| column.width).collect());
        for file in selected {
            let request = RecordsListRequest {
                file: Some(file),
                signature: Some(self.kind.signature().to_owned()),
                offset: 0,
                // `records.list` defaults to 100 records; the spreadsheet
                // lists every record of the group.
                limit: Some(usize::MAX),
            };
            let list: RecordList = match bridge.request("records.list", &request) {
                Ok(list) => list,
                Err(error) => {
                    self.error = Some(error.message.clone());
                    return Err(error);
                }
            };
            for summary in list.records {
                let request = RecordsGetRequest {
                    form_id: summary.form_id.clone(),
                    file: Some(summary.file.clone()),
                    depth: None,
                };
                match bridge.request::<_, RecordDetail>("records.get", &request) {
                    Ok(detail) => self.rows.push(SpreadsheetRow {
                        record: summary,
                        detail,
                    }),
                    Err(error) => self.error = Some(error.message),
                }
            }
        }
        self.selected.clear();
        self.focus = None;
        self.loaded = true;
        Ok(())
    }

    pub fn selected_rows(&self) -> Vec<&SpreadsheetRow> {
        self.selected.iter().filter_map(|index| self.rows.get(*index)).collect()
    }

    /// `vstSpreadSheetCompareNodes`: a header click sorts the rows by the
    /// text of the column (`CompareText`), a Control click reversing it.
    pub fn sort_by_column(&mut self, column: usize, descending: bool) {
        let Some(decl) = self.decls.get(column).copied() else {
            return;
        };
        let kind = self.kind;
        let skyrim = self.skyrim;
        let mut selected: Vec<String> = self
            .selected_rows()
            .iter()
            .map(|row| row.record.form_id.clone())
            .collect();
        self.rows.sort_by(|left, right| {
            let order = compare_text(&left.text(kind, decl, skyrim), &right.text(kind, decl, skyrim));
            if descending { order.reverse() } else { order }
        });
        selected.sort();
        self.selected = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| selected.binary_search(&row.record.form_id).is_ok())
            .map(|(index, _)| index)
            .collect();
    }
}

/// The grid view of a [`SpreadsheetPane`].
struct SpreadsheetGrid<'a> {
    rows: &'a [SpreadsheetRow],
    columns: &'a [GridColumn],
    decls: &'a [usize],
    kind: SpreadsheetKind,
    skyrim: bool,
    /// `wbEditAllowed`: the spreadsheets edit only while editing is allowed.
    edit_allowed: bool,
    /// The edit the grid accepted and the command layer has yet to run
    /// (`vstSpreadSheetNewText`).
    pending_edit: Option<SpreadsheetEdit>,
}

/// The edit of a spreadsheet cell (`vstSpreadSheetNewText`): the record, the
/// path of the element and the text typed into the editor.
#[derive(Clone, Debug, PartialEq)]
pub struct SpreadsheetEdit {
    /// Load order FormID of the record.
    pub form_id: String,
    /// Plugin whose version of the record is edited.
    pub file: String,
    /// Path of the element inside the record.
    pub path: String,
    /// The text typed into the editor.
    pub value: String,
}

impl GridSource for SpreadsheetGrid<'_> {
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn columns(&self) -> &[GridColumn] {
        self.columns
    }

    fn cell(&self, row: usize, column: usize, out: &mut GridCell) {
        let Some(entry) = self.rows.get(row) else {
            return;
        };
        let Some(decl) = self.decls.get(column) else {
            return;
        };
        out.text = entry.text(self.kind, *decl, self.skyrim);
        // `vstSpreadSheetEditing`: the columns before the tree's `Tag` are
        // not edited, and only an editable element is.
        // The `Tag` of the tree is the index of the visible column, not
        // of the DFM declaration.
        if column < SPREADSHEET_EDIT_COLUMN {
            return;
        }
        let source = cell_source(self.kind, *decl, self.skyrim);
        let Some(element) = entry.element(source) else {
            return;
        };
        if !element.editable {
            return;
        }
        out.editable = self.edit_allowed;
        out.edit_text = element.edit_value.clone().or_else(|| Some(element.value.clone()));
    }

    fn set_cell(&mut self, row: usize, column: usize, text: &str) -> bool {
        if !self.edit_allowed {
            return false;
        }
        let Some(entry) = self.rows.get(row) else {
            return false;
        };
        let Some(decl) = self.decls.get(column).copied() else {
            return false;
        };
        if column < SPREADSHEET_EDIT_COLUMN {
            return false;
        }
        let source = cell_source(self.kind, decl, self.skyrim);
        let Some(element) = entry.element(source) else {
            return false;
        };
        if !element.editable {
            return false;
        }
        let Some(path) = entry.path(source) else {
            return false;
        };
        self.pending_edit = Some(SpreadsheetEdit {
            form_id: entry.record.form_id.clone(),
            file: entry.record.file.clone(),
            path,
            value: text.to_owned(),
        });
        true
    }
}

/// The first column of a spreadsheet that is edited: the `Tag` of the
/// spreadsheet trees (`xeMainForm.dfm`), which upstream compares the
/// column index against (`vstSpreadSheetEditing`).
pub const SPREADSHEET_EDIT_COLUMN: usize = 3;

/// Draws a spreadsheet tab. Returns the action of `Compare Selected` (the
/// view switch) or of a click on a row (`JumpTo`).
pub fn spreadsheet_pane(
    ui: &mut egui::Ui,
    pane: &mut SpreadsheetPane,
    bridge: &mut CommandBridge,
    files: &[String],
    skyrim: bool,
    colors: &crate::widgets::conflict::ConflictColors,
    edit_allowed: bool,
) -> PaneAction {
    let mut action = PaneAction::None;
    if !pane.loaded {
        let _ = pane.load(bridge, files, skyrim);
    }
    ui.horizontal(|ui| {
        ui.label(pane.kind.signature());
        if ui.button("Rebuild").clicked() {
            let _ = pane.load(bridge, files, skyrim);
        }
        ui.label(format!("{} records", pane.rows.len()));
    });
    if let Some(error) = &pane.error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    let id = ui.scope_id().with(pane.kind.signature());
    let mut source = SpreadsheetGrid {
        rows: &pane.rows,
        columns: &pane.columns,
        decls: &pane.decls,
        kind: pane.kind,
        skyrim: pane.skyrim,
        edit_allowed,
        pending_edit: None,
    };
    // `pmuSpreadsheet` is the popup of the grid: the header and the cells
    // of the sheet open it.
    let mut chosen: Option<&'static str> = None;
    let mut menu = |ui: &mut egui::Ui, _is_header: bool, _column: usize| {
        chosen = crate::menu_model::draw_menu(ui, &crate::menu_model::PMU_SPREADSHEET, &|item| {
            crate::menu_model::MenuItemState {
                enabled: match item.id {
                    "mniSpreadsheetCompareSelected" => pane.selected.len() >= 2,
                    _ => item.enabled,
                },
                checked: item.checked,
            }
        })
        .map(|item| item.id);
    };
    let output = Grid {
        id,
        source: &mut source,
        state: &mut pane.state,
        colors,
        header: true,
        context_menu: Some(&mut menu),
        own_double_click: true,
    }
    .show(ui);
    // `vstSpreadSheetNewText`: the edit the grid accepted goes to
    // `elements.set` on the element of the cell.
    if let Some(edit) = source.pending_edit.take() {
        let request = ElementsSetRequest {
            form_id: edit.form_id,
            path: edit.path,
            file: Some(edit.file),
            value: Value::String(edit.value),
            dry_run: false,
        };
        match bridge.request::<_, ElementsSetResponse>("elements.set", &request) {
            Ok(response) => {
                if !response.changed {
                    action = action.merge(PaneAction::Messages(vec![format!("{} is unchanged", response.path)]));
                }
            }
            Err(error) => action = action.merge(PaneAction::Messages(vec![format!("Fatal: {}", error.message)])),
        }
    }
    // `vstSpreadSheetNewText`: the edit the grid accepted goes to
    // `elements.set` on the element of the cell.
    if let Some(edit) = source.pending_edit.take() {
        let request = ElementsSetRequest {
            form_id: edit.form_id,
            path: edit.path,
            file: Some(edit.file),
            value: Value::String(edit.value),
            dry_run: false,
        };
        match bridge.request::<_, ElementsSetResponse>("elements.set", &request) {
            Ok(response) => {
                if !response.changed {
                    action = action.merge(PaneAction::Messages(vec![format!("{} is unchanged", response.path)]));
                }
            }
            Err(error) => action = action.merge(PaneAction::Messages(vec![format!("Fatal: {}", error.message)])),
        }
    }
    if let Some((column, _)) = output.header_clicked {
        let descending = ui.input(|input| input.modifiers.ctrl);
        pane.sort_by_column(column, descending);
    }
    if let Some((row, _)) = output.clicked {
        pane.focus = Some(row);
        if ui.input(|input| input.modifiers.ctrl) {
            // `vstSpreadSheetClick`: Control jumps to the record.
            if let Some(entry) = pane.rows.get(row) {
                action = PaneAction::Jump(RecordSelection::new(
                    &entry.record.form_id,
                    Some(entry.record.file.clone()),
                ));
            }
        } else if ui.input(|input| input.modifiers.shift) {
            pane.selected.insert(row);
        } else {
            pane.selected.clear();
            pane.selected.insert(row);
        }
    }
    if let Some((row, _)) = output.secondary_clicked {
        pane.focus = Some(row);
        if !pane.selected.contains(&row) {
            pane.selected.clear();
            pane.selected.insert(row);
        }
    }
    let response = ui.response();
    let _ = response;
    match chosen {
        Some("mniSpreadsheetCompareSelected") => {
            let records: Vec<Value> = pane
                .selected_rows()
                .iter()
                .map(|row| json!({"form_id": row.record.form_id, "file": row.record.file}))
                .collect();
            if records.len() >= 2 {
                match bridge.call("records.compare_selected", json!({"records": records})) {
                    Ok(response) => {
                        action = PaneAction::ShowView;
                        let messages = response_messages(&response);
                        if !messages.is_empty() {
                            action = action.merge(PaneAction::Messages(messages));
                        }
                    }
                    Err(error) => action = PaneAction::Messages(vec![format!("Error: {}", error.message)]),
                }
            }
        }
        Some("mniSpreadsheetRebuild") => {
            let _ = pane.load(bridge, files, skyrim);
        }
        _ => {}
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, signature: &str, file: &str, form_id: &str) -> ReferencedByItem {
        ReferencedByItem {
            name: name.to_owned(),
            signature: signature.to_owned(),
            file: file.to_owned(),
            form_id: form_id.to_owned(),
            raw_file_name: file.to_owned(),
        }
    }

    #[test]
    fn the_referenced_by_filter_is_upstream_apply_referenced_by_filter() {
        let rows = [
            item("Iron Sword", "WEAP", "Skyrim.esm", "00012EB7"),
            item("Steel Sword", "WEAP", "Update.esm", "000135B8"),
            item("Breezehome", "CELL", "Skyrim.esm", "000165A8"),
        ];
        let filter = |name: &str, signature: &str, file: &str, or: bool| ReferencedByFilter {
            name: name.to_owned(),
            signature: signature.to_owned(),
            file: file.to_owned(),
            or,
        };
        // No filter: every row.
        let all = filter("", "", "", false);
        assert_eq!(rows.iter().filter(|row| all.matches(row)).count(), 3);
        // Name only, without case, a substring.
        let name = filter("SWORD", "", "", false);
        assert_eq!(rows.iter().filter(|row| name.matches(row)).count(), 2);
        // Name and signature: AND joins them.
        let and = filter("iron", "CELL", "", false);
        assert_eq!(rows.iter().filter(|row| and.matches(row)).count(), 0);
        // The same boxes joined by OR.
        let or = filter("iron", "CELL", "", true);
        assert_eq!(rows.iter().filter(|row| or.matches(row)).count(), 2);
        // The file is always AND.
        let file = filter("", "", "update", true);
        assert_eq!(rows.iter().filter(|row| file.matches(row)).count(), 1);
        // Text is trimmed before it is used.
        let trimmed = filter("  iron ", "", "", false);
        assert_eq!(rows.iter().filter(|row| trimmed.matches(row)).count(), 1);
    }

    #[test]
    fn the_sort_keys_and_the_column_click_cycle() {
        let mut pane = ReferencedByPane::new();
        pane.items = vec![
            item("b", "WEAP", "b.esm", "00000002"),
            item("A", "CELL", "a.esm", "00000001"),
        ];
        pane.sort_items();
        // `CompareText` is case insensitive.
        assert_eq!(pane.items[0].name, "A");
        // Record is sorted: the next click on Record switches to FormID.
        pane.on_column_click(0, false);
        assert_eq!(pane.sort.column, 3);
        assert_eq!(pane.column_caption(0), "Record [FormID] \u{25B2}");
        // Another column sorts by itself first; clicking it again switches
        // File to RawFileName, descending with Control.
        pane.on_column_click(2, false);
        assert_eq!(pane.sort.column, 2);
        pane.on_column_click(2, true);
        assert_eq!(pane.sort.column, 4);
        assert_eq!(pane.column_caption(2), "File [Raw] \u{25BC}");
        // Descending by the raw file name: b.esm first.
        assert_eq!(pane.items[0].form_id, "00000002");
    }

    #[test]
    fn the_tab_caption_is_update_referenced_by_tab_caption() {
        let mut pane = ReferencedByPane::new();
        pane.total = 3;
        pane.items = vec![item("a", "WEAP", "a.esm", "00000001")];
        pane.selected.insert(0);
        assert_eq!(pane.caption(false), "Referenced By (3)");
        assert_eq!(pane.caption(true), "Referenced by (3 / F: 1 / S: 1)");
        assert!(pane.visible());
        pane.total = 0;
        assert!(!pane.visible());
    }

    #[test]
    fn the_message_double_click_reads_the_form_id() {
        assert_eq!(parse_message_form_id("00012EB7").as_deref(), Some("00012EB7"));
        assert_eq!(
            parse_message_form_id("  [0000:00012EB7]  ").as_deref(),
            Some("00012EB7")
        );
        assert_eq!(
            parse_message_form_id("Something [WEAP:00012EB7] happened").as_deref(),
            Some("00012EB7")
        );
        assert_eq!(parse_message_form_id("00012EB"), None);
        assert_eq!(parse_message_form_id("NotAFormID"), None);
        assert_eq!(parse_message_form_id(""), None);
    }

    #[test]
    fn the_information_memo_is_the_dfm_text() {
        let lines: Vec<&str> = INFORMATION_TEXT.split('\n').collect();
        assert_eq!(lines.len(), 216);
        assert_eq!(
            lines[0],
            "xEdit is an advanced graphical esp editor and conflict detector."
        );
        assert_eq!(lines[2], "Discord: https://discord.gg/5t8RnNQ");
        assert!(lines[215].is_empty());
        assert!(INFORMATION_TEXT.contains("-DontCacheLoad"));
        assert!(INFORMATION_TEXT.contains("Only Master and Leafs"));
    }

    #[test]
    fn the_spreadsheet_columns_follow_the_dfm_and_the_skyrim_adjustments() {
        let oblivion = spreadsheet_columns(SpreadsheetKind::Weapon, false);
        let titles: Vec<&str> = oblivion.iter().map(|(_, title)| title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "File Name",
                "FormID",
                "EditorID",
                "Weapon Name",
                "Enchantment",
                "Type",
                "Speed",
                "Reach",
                "Value",
                "Health",
                "Weight",
                "Damage"
            ]
        );
        let skyrim = spreadsheet_columns(SpreadsheetKind::Weapon, true);
        let titles: Vec<&str> = skyrim.iter().map(|(_, title)| title.as_str()).collect();
        assert!(!titles.contains(&"Health"));
        for title in ["Amount", "Skill", "Stagger", "Crit. Damage", "Range Max", "Template"] {
            assert!(titles.contains(&title), "{title} is visible for Skyrim");
        }
        // Display order follows Position: Amount is the sixth column.
        assert_eq!(titles[5], "Amount");
        let armor = spreadsheet_columns(SpreadsheetKind::Armor, true);
        assert!(armor.iter().any(|(_, title)| title == "Armor Type"));
        assert!(armor.iter().any(|(_, title)| title == "Equip. Type"));
        let ammo = spreadsheet_columns(SpreadsheetKind::Ammo, true);
        let titles: Vec<&str> = ammo.iter().map(|(_, title)| title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "File Name",
                "FormID",
                "EditorID",
                "Ammunition Name",
                "Projectile",
                "Value",
                "Damage",
                "Flags"
            ]
        );
    }

    #[test]
    fn the_spreadsheet_cell_sources_are_the_init_node_assignments() {
        // Skyrim weapon: DATA[0] Value, DNAM[2] Speed, CRDT % Mult.
        assert_eq!(
            cell_source(SpreadsheetKind::Weapon, 8, true),
            CellSource::DataIndex("DATA", 0)
        );
        assert_eq!(
            cell_source(SpreadsheetKind::Weapon, 6, true),
            CellSource::DataIndex("DNAM", 2)
        );
        assert_eq!(
            cell_source(SpreadsheetKind::Weapon, 16, true),
            CellSource::DataNamed("CRDT", "% Mult")
        );
        // Oblivion weapon: DATA[0] Type, DATA[4] Value.
        assert_eq!(
            cell_source(SpreadsheetKind::Weapon, 5, false),
            CellSource::DataIndex("DATA", 0)
        );
        assert_eq!(
            cell_source(SpreadsheetKind::Weapon, 8, false),
            CellSource::DataIndex("DATA", 4)
        );
        // Skyrim armor: BODT[3] Armor Type, EITM Enchantment.
        assert_eq!(
            cell_source(SpreadsheetKind::Armor, 6, true),
            CellSource::DataIndex("BODT", 3)
        );
        assert_eq!(cell_source(SpreadsheetKind::Armor, 4, true), CellSource::Top("EITM"));
        // Skyrim ammo: Projectile and Flags, members of DATA.
        assert_eq!(
            cell_source(SpreadsheetKind::Ammo, 4, true),
            CellSource::DataNamed("DATA", "Projectile")
        );
        assert_eq!(
            cell_source(SpreadsheetKind::Ammo, 9, true),
            CellSource::DataNamed("DATA", "Flags")
        );
        assert_eq!(cell_source(SpreadsheetKind::Ammo, 5, true), CellSource::None);
    }
}
