// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (FormCreate, FormShow,
// tmrStartupTimer, DoInit's message log and source selection,
// WMUserLoaderDone, FormClose and SaveSettings, AddMessage,
// ScrollToTheLastMessage, bnMainMenuMouseDown, mniMessagesClearClick,
// mniMessagesSaveSelectedClick, mniMessagesAutoscroll, the Help buttons,
// mniNavOptionsClick, mniNavGenerateLODClick (the legend's button,
// bnLegend, is the step-3 view panel; the dialog itself is here),
// mniNavBuildRefClick, mniNavUndeleteAndDisableReferencesClick,
// mniNavRemoveIdenticalToMasterClick, EditWarn and the New FormID prompt)
// and xEdit/xeMainForm.dfm (pnlClient, splElements, stbMain, pnlTop with
// bnMainMenu, bnBack, bnForward, lblPath and pnlBtn, pnlNav with vstNav,
// pnlRight with pgMain: tbsView, tbsReferencedBy, tbsMessages, tbsInfo and
// the three spreadsheet tabs, and the popup menus)

//! `TfrmMain`: the main window of the GUI. The form owns the shell state
//! (the startup screen, the settings, the message log and the tab), draws
//! the top bar, the navigation pane, the tabbed view area and the status
//! bar, and runs the startup load on a worker thread.
//!
//! The navigation tree, the view and the record panes are filled in, and
//! the context menus of the tree, the view and the panes are attached to
//! them.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use eframe::egui;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use xedit_core::interface::globals::{self, GameMode, data_path, my_games_the_game_path};
use xedit_core::interface::misc::str_to_int_def;
use xedit_session::commands::{ElementDetail, ElementsGetRequest, ElementsSetRequest, ElementsSetResponse};
use xedit_session::conflicts::{
    CompareRequest, CompareResponse, CompareSelectedRequest, CompareSelectedResponse, SelectedRecord,
};
use xedit_session::filter::{
    FilterApplyRequest, FilterPresetDeleteRequest, FilterPresetSaveRequest, FilterPresetsRequest,
    FilterPresetsResponse, SignaturesListRequest, SignaturesListResponse,
};
use xedit_session::localization::{
    LocalizationExportRequest, LocalizationFilesRequest, LocalizationFilesResponse, LocalizationSetRequest,
    LocalizationSetResponse, LocalizationStringsRequest, LocalizationStringsResponse, LocalizationSwitchRequest,
    LocalizationSwitchResponse,
};
use xedit_session::log_analyzer::{LogType, LogsAnalyzeRequest, LogsAnalyzeResponse};
use xedit_session::modgroups::{
    ChangeResponse as ModGroupChangeResponse, EditRequest as ModGroupEditRequest, ListRequest as ModGroupListRequest,
    ListResponse as ModGroupListResponse, PresetDeleteRequest as ModGroupPresetDeleteRequest,
    PresetSaveRequest as ModGroupPresetSaveRequest, PresetsRequest as ModGroupPresetsRequest,
    PresetsResponse as ModGroupPresetsResponse, SelectRequest as ModGroupSelectRequest,
    ShowRequest as ModGroupShowRequest, ShowResponse as ModGroupShowResponse,
};
use xedit_session::module_select::{
    ModulesListRequest, ModulesListResponse, PresetDeleteRequest as ModulePresetDeleteRequest,
    PresetLoadRequest as ModulePresetLoadRequest, PresetLoadResponse as ModulePresetLoadResponse,
    PresetSaveRequest as ModulePresetSaveRequest, PresetsRequest as ModulePresetsRequest,
    PresetsResponse as ModulePresetsResponse,
};
use xedit_session::script::{
    ScriptListRequest, ScriptListResponse, ScriptReadRequest, ScriptReadResponse, ScriptRunRequest, ScriptRunResponse,
    ScriptSaveRequest, ScriptSaveResponse,
};
use xedit_session::settings::{SettingsGetRequest, SettingsGetResponse, SettingsSetRequest, SettingsSetResponse};

use xedit_core::interface::types::{ConflictAll, ConflictThis};
use xedit_session::{CommandError, Session};

use crate::command::{self, CommandBridge};
use crate::dialogs::developer_message_form::{DeveloperMessageAction, DeveloperMessageState};
use crate::dialogs::edit_warning_form::{self, EditWarningAction, EditWarningState};
use crate::dialogs::file_select_form::{self, FileSelectAction, FileSelectState};
use crate::dialogs::filter_options_form::{
    self as filter_options_form, FilterOptionsAction, FilterOptionsState, FilterQuestion,
};
use crate::dialogs::form_id_prompt::{self, FormIdAction, FormIdState};
use crate::dialogs::legend_form::{self, LegendAction, LegendState};
use crate::dialogs::localization_form::{LocalizationAction, LocalizationState};
use crate::dialogs::localize_plugin_form::{LocalizePluginAction, LocalizePluginState};
use crate::dialogs::lodgen_form::{self, LodgenAction, LodgenState};
use crate::dialogs::log_analyzer_form::{LogAnalyzerAction, LogAnalyzerState};
use crate::dialogs::mod_group_edit_form::{self as mod_group_edit_form, ModGroupEditAction, ModGroupEditState};
use crate::dialogs::mod_group_select_form::{self as mod_group_select_form, ModGroupSelectAction, ModGroupSelectState};
use crate::dialogs::module_select_form::{self as module_select_form, ModuleSelectAction};
use crate::dialogs::options_form::{self, OptionsAction, OptionsState};
use crate::dialogs::path_prompt::{self as path_prompt, PathPromptAction, PathPromptState};
use crate::dialogs::prompts::{self, PendingPrompt, PromptAnswer, PromptQueue, PromptRequest};
use crate::dialogs::rich_edit_form::{RichEditAction, RichEditState};
use crate::dialogs::script_form::{ScriptFormAction, ScriptFormState};
use crate::dialogs::selection::DialogPlacement;
use crate::dialogs::text_prompt::{self as text_prompt, TextPromptAction, TextPromptState};
use crate::dialogs::tip_form::{TipAction, TipState};
use crate::dialogs::view_elements_form::{ElementTab, ViewElementsAction, ViewElementsState};
use crate::dialogs::wait_form::{self as wait_form, WaitFormAction, WaitFormState};
use crate::dialogs::worldspace_cell_details_form::{
    self as worldspace_cell_details_form, WorldspaceCellDetailsAction, WorldspaceCellDetailsState,
};
use crate::dialogs::{DialogHost, DialogKind, setting_get_bool, setting_get_integer, setting_get_string, setting_set};
use crate::element_detail::ElementDetailPane;
use crate::job_runner::{FinishedJob, JobRunner, JobSpec};
use crate::menu_model::{self, MenuItem, MenuKind, MenuTree};
use crate::nav_flows::{
    CopyIntoKind as NavCopyIntoKind, CopyIntoRepeat, NavAnswer, NavAsk, NavDialog, NavFlow, add_master_candidates,
    batch_pairs, build_ref_candidates, change_form_id_candidates, copy_into_repeat, copy_target_candidates,
    file_template, file_templates, parse_batch_csv, update_crc_candidates,
};
use crate::nav_tree::{NavAction, NavSource, SearchState};
use crate::panes::{
    self, LocalizationPane, MessagesPane, PaneAction, RecordSelection, ReferencedByPane, SpreadsheetKind,
    SpreadsheetPane,
};
use crate::screenshot::{ScreenshotOutcome, ScreenshotState};
use crate::selection::{NodeKind, Selection};
use crate::settings::{GuiSettings, NAV_COLUMN_WIDTHS, WindowState};
use crate::startup::{ModuleSelectMode, ModuleSelectState, Startup, application_title};
use crate::view_grid::{PendingEdit, ViewFilterState, ViewGrid, ViewKind, ViewModel, ViewRecord, ViewSource};
use crate::widgets::grid::Grid;
use crate::widgets::virtual_tree::{TreeSource, TreeState, TreeView};
use crate::widgets::{MenuCommand, apply_font_and_scale, vcl_styles_system_menu};

/// The tabs of `pgMain`, in DFM order (`TabPosition = tpBottom`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    View,
    ReferencedBy,
    Messages,
    Information,
    Weapon,
    Armor,
    Ammunition,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::View,
        Tab::ReferencedBy,
        Tab::Messages,
        Tab::Information,
        Tab::Weapon,
        Tab::Armor,
        Tab::Ammunition,
    ];

    pub fn caption(self) -> &'static str {
        match self {
            Tab::View => "View",
            Tab::ReferencedBy => "Referenced By",
            Tab::Messages => "Messages",
            Tab::Information => "Information",
            Tab::Weapon => "Weapon Spreadsheet",
            Tab::Armor => "Armor Spreadsheet",
            Tab::Ammunition => "Ammunition Spreadsheet",
        }
    }
}

/// A menu item the user clicked: the DFM name of the item and the action it
/// names. The form runs it after the menu has closed.
pub type ChosenMenu = (&'static str, MenuCommand);

/// A copy-into request of `pmuViewHeader` waiting for its target plugin: the
/// small stand-in for `xeFileSelectForm` that step 5 replaces.
#[derive(Clone, Debug, PartialEq)]
pub struct CopyInto {
    /// The record of the view column the header was opened on.
    pub record: ViewRecord,
    /// Which copy item was chosen (`mniViewHeaderCopyIntoClick`).
    pub kind: CopyIntoKind,
    /// Plugin to copy into (`CopyInto`'s `TfrmFileSelect`).
    pub target: String,
    /// `Copy as wrapper` asks for the wrapper's editor ID.
    pub wrapper_editor_id: String,
}

/// The copy items of `pmuViewHeader` (`mniViewHeaderCopyIntoClick`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyIntoKind {
    /// `mniViewHeaderCopyAsOverride`.
    Override,
    /// `mniViewHeaderCopyAsOverrideWithOverwriting`.
    OverrideOverwrite,
    /// `mniViewHeaderDeepCopyAsOverride`.
    DeepOverride,
    /// `mniViewHeaderDeepCopyAsOverrideWithOverwriting`.
    DeepOverrideOverwrite,
    /// `mniViewHeaderCopyAsNewRecord`.
    AsNew,
    /// `mniViewHeaderCopyAsWrapper`.
    AsWrapper,
}

impl CopyIntoKind {
    /// The `records.copy` request fields this item sets.
    fn request(self) -> (bool, bool, bool, bool) {
        // (as_new, deep, overwrite, as_wrapper)
        match self {
            CopyIntoKind::Override => (false, false, false, false),
            CopyIntoKind::OverrideOverwrite => (false, false, true, false),
            CopyIntoKind::DeepOverride => (false, true, false, false),
            CopyIntoKind::DeepOverrideOverwrite => (false, true, true, false),
            CopyIntoKind::AsNew => (true, false, false, false),
            CopyIntoKind::AsWrapper => (false, false, false, true),
        }
    }
}

/// A mod group creation waiting for its name: the small stand-in for
/// `xeModGroupEditForm` that step 5 replaces.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateModGroup {
    pub name: String,
}

/// `Some(text)` for a filter box that has text, `None` for an empty one.
fn non_empty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_owned())
}

/// The startup screen of the current frame, copied out of [`Startup`] so
/// the drawing methods can take `&mut self`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    GameSelect,
    ModuleSelect,
    Loading,
    Failed,
    Loaded,
}

/// `lblFilterHint.Caption` (`xeMainForm.dfm`, spelling kept).
const FILTER_HINT: &str = "A filter has been applied. The treeview contents is fossilized and will not adjust structure to changes.  Please remove or re-apply the filter if necessary.";

/// The dialogs, one at a time (they are modal upstream).
pub enum Dialog {
    ModGroupSelect(Box<ModGroupSelectState>),
    ModGroupEdit(Box<ModGroupEditState>),
    FilterOptions(Box<FilterOptionsState>),
    FileSelect(Box<FileSelectState>),
    /// `mniNavOptionsClick`: `TfrmOptions`.
    Options(Box<OptionsState>),
    /// `mniNavGenerateLODClick`: `TfrmLODGen`.
    Lodgen(Box<LodgenState>),
    /// `bnLegendClick` (`TfrmLegend`).
    Legend(Box<LegendState>),
    /// `EditWarn` (`TfrmEditWarning`).
    EditWarning(Box<EditWarningState>),
    /// `InputQuery`: the one-line box of the nav flows (`mniNavChangeFormIDClick`,
    /// `mniNavRenumberFormIDsFromClick`, `CopyInto` and the rest).
    TextPrompt(Box<TextPromptState>),
    /// `TOpenDialog`: the file of the disk of the nav flows (`mniNavCompareToClick`,
    /// `mniNavCreateDeltaPatchClick`, `mniNavBatchChangeReferencingRecordsClick`).
    PathPrompt(Box<PathPromptState>),
}

/// The flow that is running, its asks and the answers so far.
pub struct NavFlowState {
    /// The flow the item names.
    pub flow: NavFlow,
    /// The dialogs the flow opens, in the order upstream asks them.
    pub asks: Vec<NavAsk>,
    /// The answers, one per ask that was answered.
    pub answers: Vec<NavAnswer>,
    /// The ask that is open.
    pub index: usize,
}

/// The yes/no question of a flow.
pub struct NavQuestionState {
    /// The text of the question.
    pub message: String,
    /// Whether the answer that is taken when the question is closed is
    /// yes.
    pub default_yes: bool,
}

/// The command a finished job answered.
pub enum PendingJob {
    /// A menu command with no dialog: its messages go to the log.
    Command,
    /// `filter.apply` of `TfrmFilterOptions`: the counts and the placement.
    Filter { rect: Option<egui::Rect> },
    /// The dry run of `lodgen.generate`: the response fills `TfrmLODGen`.
    LodgenChecklist,
    /// The run of `TfrmLODGen`: its messages go to the log.
    LodgenRun,
}

/// The question of a core callback that waits for its dialog.
pub enum PromptState {
    /// `wbGetFormIDCallback`.
    FormId { pending: PendingPrompt, state: FormIdState },
    /// `wbGetCellDetailsForWorldspaceCallback`.
    CellDetails {
        pending: PendingPrompt,
        state: WorldspaceCellDetailsState,
        worldspace: String,
    },
}

/// A `MessageDlg` in front of the actions of a menu item: Remove asks
/// before it deletes, Compact FormIDs before it renumbers.
pub struct ConfirmState {
    pub message: String,
    pub actions: Vec<NavAction>,
}

/// The main form.
pub struct TfrmMain {
    /// `Application.Title` and the native window data.
    pub program: PathBuf,
    pub settings: GuiSettings,
    pub startup: Startup,
    pub game: Option<GameMode>,
    pub bridge: Option<CommandBridge>,
    /// `mmoMessages` and `pmuMessages`.
    pub messages: MessagesPane,
    /// `tbsReferencedBy`: `lvReferencedBy` and its filter strip.
    pub referenced_by: ReferencedByPane,
    /// `Memo1` of `tbsInfo`.
    pub information_selection: String,
    /// The localization view of the active record.
    pub localization: LocalizationPane,
    /// The items of the menus upstream fills at run time: the language
    /// list of `mniMainLocalizationLanguage` (`pmuMainPopup`).
    language_menu: Vec<menu_model::RuntimeMenu>,
    /// Whether the language list was asked for already; it is asked for
    /// again after the load and after every switch.
    language_menu_loaded: bool,
    /// `tbsWEAPSpreadsheet`, `tbsARMOSpreadsheet` and
    /// `tbsAMMOSpreadsheet`, in that order.
    pub spreadsheets: [SpreadsheetPane; 3],
    /// The element detail editor (`xeElementDetailForm`'s rules).
    pub element_detail: ElementDetailPane,
    /// `DoSetActiveRecord`: the record the panes follow.
    pub active_record: Option<RecordSelection>,
    /// `XEDIT_GUI_TAB`: the tab the first load ends on.
    pub initial_tab: Option<Tab>,
    pub active_tab: Tab,
    pub nav: NavSource,
    pub nav_state: TreeState,
    /// `vstNav.GetSortedSelection(True)` and the focused node, for the view
    /// grid (step 3) and the menus.
    pub selection: Selection,
    /// The open confirmation dialog.
    pub confirm: Option<ConfirmState>,
    pub files: Vec<String>,
    pub data_path: String,
    /// The load order file id of each file of `files` (`session.info`): the
    /// high byte of the load order FormID of its records, -1 for the
    /// hardcoded executable.
    pub file_ids: Vec<i32>,
    /// `mniBtnShrinkButtons`.
    pub shrink_buttons: bool,
    /// The radio entries of `mniViewStick*` (`Auto Top Row` by default).
    pub stick_follows_selection: bool,
    /// The view tab: the record grid, its focus and its in-place edit
    /// (`vstView`).
    pub view: ViewGrid,
    /// The records the navigation tree's selection shows in the view. Step
    /// 2's `Selection` model calls [`TfrmMain::set_active_records`]; this is
    /// the minimal interface the view consumes (`SetActiveRecord`).
    pub view_selection: Vec<ViewRecord>,
    /// `History`: the selections `bnBack`/`bnForward` walk.
    pub history: Vec<Vec<ViewRecord>>,
    /// The position of `view_selection` in `history`.
    pub history_position: usize,
    /// `fpnlViewFilter`: `edViewFilterName`, `cobViewFilter`,
    /// `edViewFilterValue` and the three keep boxes.
    pub view_filter: ViewFilterState,
    /// `bnPinned.Down`: the view stays on the record it shows while the
    /// navigation tree moves on (`IsPinned`).
    pub pinned: bool,
    /// The `lblPath` text of the moment the view was pinned: upstream
    /// leaves the label alone while the pin is on.
    pub pinned_path: String,
    /// `LockOutPinnedCount`: the history entries lock the pin out while
    /// they show their records (`LockOutPinned`/`UnLockOutPinned`).
    pub pin_lockout: usize,
    /// `mniMasterAndLeafs.Checked`.
    pub view_master_and_leafs: bool,
    /// `mniModGroupsEnabled.Checked`: compare with the saved mod group
    /// selection active.
    pub view_mod_groups: bool,
    /// Whether the session has mod groups (`wbModGroupsByName`), which the
    /// navigation popup shows the mod group items for.
    pub mod_groups_present: bool,
    /// The header column `pmuViewHeader` was opened on (`PopupIndex`).
    pub view_header_column: Option<usize>,
    /// A copy-into request waiting for its target file: a small stand-in
    /// for `xeFileSelectForm` (step 5).
    pub copy_into: Option<CopyInto>,
    /// A record removal waiting for its confirmation (`MessageDlg`).
    pub confirm_delete: Option<ViewRecord>,
    /// A mod group creation waiting for its name: a small stand-in for
    /// `xeModGroupEditForm` (step 5).
    pub create_mod_group: Option<CreateModGroup>,
    /// The reason the game selection refused a mode.
    pub game_select_error: Option<String>,
    /// The reason a module selection action refused.
    pub module_error: Option<String>,
    /// The search text and check state of the module dialog.
    pub module_select: Option<Box<ModuleSelectState>>,
    /// A session over no files, which serves the startup module dialog's
    /// `modules.list` and preset commands before the load.
    pub module_bridge: Option<CommandBridge>,
    /// The dialog of step 5 that is open.
    pub dialog: Option<Dialog>,
    /// `mniNavEditModGroupClick`'s two dialogs: the selection opens the
    /// edit form instead of activating the groups.
    pub edit_group_flow: bool,
    /// The step-6 dialogs.
    pub dialogs: DialogHost,
    /// `--dialog <name>`: the dialog the harness asks to open once loaded.
    /// The names of [`DialogKind`] open a step-6 viewer or editor; the
    /// step-5 selection dialogs are opened by [`TfrmMain::open_pending_dialog`].
    pub pending_dialog: Option<String>,
    /// `--dialog-arg <name>`: the mod group the edit dialog opens for.
    pub dialog_arg: Option<String>,
    /// `--dialog` draws a step-6 viewer or editor alone (the phase-7 screen
    /// check), without the shell's panels behind it.
    pub dialog_only: bool,
    /// `--screenshot <path>`: the app's own capture of the window, written
    /// as PNG through egui's screenshot path (see `screenshot.rs`).
    pub screenshot: Option<ScreenshotState>,
    /// The load running on its worker thread.
    load_rx: Option<Receiver<Result<Session, CommandError>>>,
    /// The callback that was installed before the load (restored after it).
    previous_callback: Option<xedit_core::interface::misc::ProgressCallback>,
    load_started: Instant,
    theme_applied: bool,
    window_placed: bool,
    /// Whether the native window's title and icon have been sent. Both go
    /// through `ViewportCommand`, and every `send_viewport_cmd` requests a
    /// repaint of its viewport (`Context::send_viewport_cmd_to`): sending
    /// them once per pass repaints the window forever.
    window_titled: bool,
    /// The screen the last frame logged under `XEDIT_GUI_LOG`.
    logged_screen: Option<&'static str>,
    /// The frames drawn, for the periodic geometry line.
    frame_count: u64,
    /// Whether edits are allowed (`--edit`; the GUI's default, off with
    /// `--view`).
    pub edit_allowed: bool,
    /// Whether the settings were written at the close request.
    settings_saved: bool,
    pub closing: bool,
    /// The rects of the shell's panels, for the layout test.
    #[cfg(test)]
    pub rects: Vec<(&'static str, egui::Rect)>,
    /// The command of the current job (`crate::job_runner`).
    pub job: JobRunner,
    pending_job: PendingJob,
    /// The questions of the two core callbacks (`crate::dialogs::prompts`).
    prompts: Arc<PromptQueue>,
    prompt: Option<PromptState>,
    /// `TfrmMain.EditWarnOk`: the warning was answered once this run.
    pub edit_warn_ok: bool,
    /// `ColumnWidth` and `RowHeight` of `[Options]`, which the tree and the
    /// view use (`mniNavOptionsClick`).
    pub column_width: i32,
    pub row_height: i32,
    /// The wait form of the startup load (`start_load`).
    load_wait: Option<Arc<WaitFormState>>,
    /// The last update of a wait form, for `fwDoUpdate`'s rate.
    last_wait_update: Instant,
    /// Whether Shift is held (`btnSplitTreesLOD.Visible := GetKeyState(
    /// VK_SHIFT) < 0`).
    pub shift_held: bool,
    /// `XEDIT_GUI_AUTOCANCEL_MS`: the screen check presses the wait form's
    /// Cancel once after this delay (the private desktop does not deliver
    /// the click to egui). Screen-check only.
    auto_cancel: Option<Instant>,
    /// The nav flow that is running (the F9 items that open a dialog
    /// before their command).
    pub nav_flow: Option<NavFlowState>,
    /// The yes/no question a flow is waiting for.
    pub nav_question: Option<NavQuestionState>,
    /// Whether the open question is the copy flow's warning of an
    /// unchanged EditorID (`CopyInto`, xeMainForm.pas:3499-3506): the
    /// copy continues when the user confirms it, and the EditorID boxes
    /// are asked again when they do not.
    nav_question_is_copy: bool,
    /// Whether the file list a flow opened takes one name only
    /// (`TfrmModuleSelect`'s `MaxSelect` of one).
    nav_flow_single: bool,
    /// The masters of `mniNavAddMastersClick` that wait for the edit
    /// warning upstream asks before the first edit.
    pending_masters: Option<(String, Vec<String>)>,
}

impl TfrmMain {
    /// `Application.CreateForm(TfrmMain, frmMain)`: the DFM defaults, the
    /// settings read once a game is chosen (`DoInit`), and the startup
    /// screen. A game named on the command line skips `TfrmGameSelect`.
    pub fn new(game: Option<GameMode>, plugins: Vec<String>, edit: bool) -> Self {
        // `TfrmMain.FormCreate`: `wbDisplayLoadOrderFormID := True`, the
        // spreadsheet's FormID column and every `FormID.ToString(True)`.
        xedit_core::interface::globals::set_pretty_form_id(true);
        let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("xedit-gui.exe"));
        let mut form = Self {
            program,
            settings: GuiSettings::default(),
            startup: Startup::game_select(),
            game: None,
            // The dialogs that need no loaded plugins (the script list, the
            // RTF viewers, the tips) answer over an empty session; the load
            // replaces it.
            bridge: Some(CommandBridge::new(Session::default())),
            dialogs: DialogHost::default(),
            pending_dialog: None,
            dialog_only: false,
            messages: MessagesPane::new(),
            referenced_by: ReferencedByPane::new(),
            information_selection: String::new(),
            localization: LocalizationPane::default(),
            language_menu: Vec::new(),
            language_menu_loaded: false,
            spreadsheets: [
                SpreadsheetPane::new(SpreadsheetKind::Weapon),
                SpreadsheetPane::new(SpreadsheetKind::Armor),
                SpreadsheetPane::new(SpreadsheetKind::Ammo),
            ],
            element_detail: ElementDetailPane::new(),
            active_record: None,
            initial_tab: None,
            active_tab: Tab::Messages,
            nav: NavSource::new(Vec::new()),
            nav_state: TreeState::new(NAV_COLUMN_WIDTHS.to_vec()),
            selection: Selection::default(),
            confirm: None,
            files: Vec::new(),
            file_ids: Vec::new(),
            data_path: String::new(),
            shrink_buttons: false,
            stick_follows_selection: false,
            view: ViewGrid::new(),
            view_selection: Vec::new(),
            history: Vec::new(),
            history_position: 0,
            view_filter: ViewFilterState::default(),
            pinned: false,
            pinned_path: String::new(),
            pin_lockout: 0,
            view_master_and_leafs: false,
            view_mod_groups: false,
            mod_groups_present: false,
            view_header_column: None,
            copy_into: None,
            confirm_delete: None,
            create_mod_group: None,
            game_select_error: None,
            module_error: None,
            module_select: None,
            module_bridge: None,
            dialog: None,
            edit_group_flow: false,
            dialog_arg: None,
            screenshot: None,
            load_rx: None,
            previous_callback: None,
            load_started: Instant::now(),
            theme_applied: false,
            window_placed: false,
            window_titled: false,
            logged_screen: None,
            frame_count: 0,
            edit_allowed: edit,
            settings_saved: false,
            closing: false,
            #[cfg(test)]
            rects: Vec::new(),
            job: JobRunner::new(),
            pending_job: PendingJob::Command,
            prompts: Arc::new(PromptQueue::new()),
            prompt: None,
            edit_warn_ok: false,
            column_width: 200,
            row_height: 19,
            load_wait: None,
            last_wait_update: Instant::now(),
            shift_held: false,
            auto_cancel: std::env::var("XEDIT_GUI_AUTOCANCEL_MS")
                .ok()
                .and_then(|text| text.parse::<u64>().ok())
                .map(|millis| Instant::now() + std::time::Duration::from_millis(millis)),
            nav_flow: None,
            nav_question: None,
            nav_question_is_copy: false,
            nav_flow_single: false,
            pending_masters: None,
        };
        // `XEDIT_GUI_TAB`: the tab the window opens on, for the screen
        // checks (the hidden desktop delivers no mouse input to the window).
        if let Ok(tab) = std::env::var("XEDIT_GUI_TAB") {
            form.initial_tab = Some(match tab.to_ascii_lowercase().as_str() {
                "view" => Tab::View,
                "referenced-by" | "referencedby" => Tab::ReferencedBy,
                "information" | "info" => Tab::Information,
                "weapon" => Tab::Weapon,
                "armor" => Tab::Armor,
                "ammunition" | "ammo" => Tab::Ammunition,
                _ => Tab::Messages,
            });
            form.active_tab = form.initial_tab.unwrap_or(Tab::Messages);
        }
        // The two core callbacks answer through this form's frame loop.
        prompts::set_ui_thread();
        prompts::install(&form.prompts);
        if let Some(game) = game {
            form.choose_game(game);
            if plugins.is_empty() {
                // `DoInit`'s source selection: the module dialog.
                form.open_module_select(game);
            } else {
                form.start_load(plugins);
            }
        }
        form
    }

    /// The settings and the title of a chosen game (`DoInit` and
    /// `Application.Title`).
    pub fn choose_game(&mut self, game: GameMode) {
        self.game = Some(game);
        let path = GuiSettings::default_path(game, &self.program);
        self.settings = GuiSettings::load(&path).unwrap_or_default();
        self.settings.path = Some(path);
        self.nav_state.column_widths = self.settings.nav_column_widths.clone();
        // `FormShow`: the `[Options]` keys of the tree.
        self.nav.show_file_flags = self.settings.options.show_file_flags;
        self.nav.show_group_record_count = self.settings.options.show_group_record_count;
        // `wbShrinkButtons` is read from the settings file, so the project
        // buttons start shrunk when the last run left them so.
        self.shrink_buttons = self.settings.options.shrink_buttons;
        // `xeMainForm.pas:5666`: `wbManualCleaningHide` and
        // `wbManualCleaningAllow` are read from the settings file; they
        // pick the manual cleaning items of the navigation popup.
        globals::set_manual_cleaning_hide(self.settings.options.manual_cleaning_hide);
        globals::set_manual_cleaning_allow(self.settings.options.manual_cleaning_allow);
        self.theme_applied = false;
        self.add_message(&format!("{} starting session", application_title(game)));
    }

    /// `AddMessage`: one line per CRLF part, the status bar takes the last.
    pub fn add_message(&mut self, text: &str) {
        self.messages.add(text);
        if self.active_tab != Tab::Messages {
            self.messages.highlighted = true;
        }
    }

    pub fn status_text(&self) -> String {
        self.messages.last()
    }

    /// `DoSetActiveRecord`: the record the Referenced By, Information,
    /// localization and element detail panes follow, from the navigation
    /// selection or a pane's jump. The view grid keeps its own selection
    /// (step 3), so this is the panes' half of the active record.
    pub fn set_pane_record(&mut self, selection: Option<RecordSelection>) {
        if self.active_record == selection {
            return;
        }
        self.active_record = selection.clone();
        self.element_detail.select(None);
        self.localization.clear();
        // `pgMainChange` fills the Referenced By list when the tab is shown;
        // `refs.get` builds the index then, so a selection does not freeze
        // the window while the references of a load order are indexed.
        self.referenced_by.clear();
        self.referenced_by.reload = selection.is_some();
    }

    /// The record of the shared navigation selection ([`Selection`]), when
    /// it names one: the focused node first, else the first selected record.
    fn selection_record(&self) -> Option<RecordSelection> {
        let node = self
            .selection
            .focused
            .as_ref()
            .filter(|node| node.is_record())
            .or_else(|| self.selection.nodes.iter().find(|node| node.is_record()))?;
        let (form_id, file) = node.record()?;
        Some(RecordSelection::new(form_id, file))
    }

    /// Points the element detail editor at an element of the active record,
    /// as the view grid's selection does (step 3).
    pub fn select_element(&mut self, path: impl Into<String>) {
        let Some(record) = self.active_record.clone() else {
            return;
        };
        self.element_detail.select(Some(crate::element_detail::SelectedElement {
            record,
            path: path.into(),
        }));
    }

    /// Starts the worker-thread load of `Session::load` (the CLI's
    /// `engine::open_session`), with the progress messages of the load kept
    /// for the message log.
    pub fn start_load(&mut self, plugins: Vec<String>) {
        let Some(game) = self.game else {
            return;
        };
        // `--load` may name a plugin of the data folder instead of a full
        // path (`Session::load` takes the paths the module dialog returns).
        let data =
            xedit_session::game_paths::data_path_of(&xedit_session::game_paths::discover_game_folder(game), game);
        let plugins: Vec<String> = plugins
            .into_iter()
            .map(|plugin| {
                if std::path::Path::new(&plugin).is_file() {
                    plugin
                } else {
                    format!("{data}{plugin}")
                }
            })
            .collect();
        let tag = xedit_session::dump::game_tag(game).to_owned();
        let previous = xedit_core::interface::misc::progress_callback();
        // The load runs off the UI thread and shows `TfrmWait` with its
        // progress messages and a Cancel that `TLoaderThread` polls.
        let wait = Arc::new(WaitFormState::new(
            "Loading",
            "Loading plugins. Please Wait...",
            true,
            std::time::Duration::ZERO,
            std::time::Duration::from_millis(100),
        ));
        let sink = Arc::clone(&wait);
        xedit_core::interface::misc::set_progress_callback(Some(Arc::new(move |message: &str| {
            sink.add_message(message);
        })));
        self.previous_callback = previous;
        self.load_wait = Some(Arc::clone(&wait));
        self.last_wait_update = Instant::now();
        self.load_started = Instant::now();
        self.startup = Startup::Loading;
        let edit = self.edit_allowed;
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("xedit-load".to_owned())
            .stack_size(xedit_core::threads::STACK_SIZE)
            .spawn(move || {
                let result = command::load_session(&tag, &plugins, edit);
                let _ = tx.send(result);
            });
        match spawned {
            Ok(_) => self.load_rx = Some(rx),
            Err(error) => {
                self.finish_load(Err(CommandError::new("internal", error.to_string())));
            }
        }
    }

    /// Drains the progress messages of a running load.
    fn drain_progress(&mut self) {
        let Some(wait) = &self.load_wait else {
            return;
        };
        for message in wait.take_messages() {
            self.add_message(&message);
        }
    }

    /// A `--screenshot` run's frame, captured through the app's own egui
    /// screenshot path. `frame_drawn` counts the painted frames the switch
    /// `--screenshot-frames` waits for; a hidden window's logic ticks pass
    /// `false`, so the count is of frames that were really drawn.
    fn poll_screenshot(&mut self, ctx: &egui::Context, frame_drawn: bool) {
        let Some(shot) = self.screenshot.as_mut() else {
            return;
        };
        // The screen the capture waits for: the load has finished (or
        // failed), or a startup dialog is up. `finish_load` opens
        // `--dialog` before it sets `Startup::Loaded`, so a loaded screen
        // with a dialog requested already has that dialog open.
        let ready = !matches!(self.startup, Startup::Loading);
        let (outcome, exit) = (shot.poll(ctx, ready, frame_drawn), shot.exit_when_done());
        if matches!(outcome, ScreenshotOutcome::Finished | ScreenshotOutcome::TimedOut) && exit {
            // `--exit`: close the window once the PNG is on disk (or the
            // screen never came), so a screen-check run cannot hang.
            self.closing = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// `WMUserLoaderDone`: the session is loaded, or the load failed.
    fn poll_load(&mut self) {
        let Some(rx) = &self.load_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.load_rx = None;
                self.finish_load(result);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.load_rx = None;
                self.finish_load(Err(CommandError::new(
                    "internal",
                    "the load thread ended without a result",
                )));
            }
        }
    }

    fn finish_load(&mut self, result: Result<Session, CommandError>) {
        // The callback served this load only.
        xedit_core::interface::misc::set_progress_callback(self.previous_callback.take());
        self.drain_progress();
        self.load_wait = None;
        match result {
            Ok(session) => {
                let mut bridge = CommandBridge::new(session);
                match bridge
                    .request::<_, xedit_session::commands::SessionInfo>("session.info", &xedit_session::NoParams {})
                {
                    Ok(info) => {
                        self.data_path = info.data_path.clone();
                        self.files = info.files.clone();
                        self.file_ids = info.file_ids.clone();
                        self.add_message(&format!("Using {} Data Path: {}", info.game_name, info.data_path));
                        self.add_message(&format!(
                            "Using {} plugins: {}",
                            info.files.len(),
                            info.files.join(", ")
                        ));
                    }
                    Err(error) => self.add_message(&format!("Fatal: {}", error.message)),
                }
                if let Some(path) = self.settings.path.as_ref() {
                    self.add_message(&format!("Using settings file: {}", path.display()));
                }
                self.add_message(&format!(
                    "Loaded {} plugins in {:.1} s",
                    self.files.len(),
                    self.load_started.elapsed().as_secs_f32()
                ));
                // `AddFileInternal` per loaded file and `DoInit`'s tree
                // options: the navigation tree starts again from the files.
                self.nav = NavSource::from_loaded();
                self.nav.show_file_flags = self.settings.options.show_file_flags;
                self.nav.show_group_record_count = self.settings.options.show_group_record_count;
                self.nav.sort_roots(self.settings.view.files_sort);
                self.nav_state.selected.clear();
                self.nav_state.focused = None;
                self.nav_state.anchor = None;
                self.selection = Selection::default();
                self.bridge = Some(bridge);
                self.startup = Startup::Loaded;
                self.active_tab = self.initial_tab.unwrap_or(Tab::Messages);
                // `pmuMainPopup` asks the session for its languages when
                // the menu opens; the list of the loaded session is asked
                // for once here.
                self.refresh_language_menu();
                // `--dialog`: the screen-check harness opens one dialog
                // without walking the menus. A step-6 viewer or editor is
                // taken by `show_dialogs` (it answers over the shell too);
                // the step-5 selection dialogs need the loaded session, so
                // they open here, and the developer message and the tip
                // wait for a later run as they do for the screen check.
                let pending_step5 = self
                    .pending_dialog
                    .as_deref()
                    .is_some_and(|name| DialogKind::from_name(name).is_none());
                if pending_step5 {
                    if let Some(dialog) = self.pending_dialog.take() {
                        self.open_pending_dialog(&dialog);
                    }
                } else {
                    self.startup_dialogs();
                }
            }
            Err(error) if error.code == "cancelled" => {
                // The wait form's Cancel stopped the load: back to the
                // module selection (`DoInit`'s source selection).
                self.add_message("Loading cancelled");
                if let Some(game) = self.game {
                    self.open_module_select(game);
                } else {
                    self.startup = Startup::game_select();
                }
            }
            Err(error) => {
                let message = format!("Fatal: {}", error.message);
                self.add_message(&message);
                self.startup = Startup::Failed(message);
            }
        }
    }

    /// `SetActiveRecord(aMainRecord)` and `SetActiveRecord(aMainRecords)`:
    /// the records the view shows. Step 2's `Selection` model calls this when
    /// the navigation tree's selection changes; the view keeps no selection
    /// of its own.
    pub fn set_active_records(&mut self, records: Vec<ViewRecord>) {
        self.remember_history(records.clone());
        self.show_records(records);
    }

    /// Shows the records without adding a history entry (`bnBack` and
    /// `bnForward`).
    fn show_records(&mut self, records: Vec<ViewRecord>) {
        self.view_selection = records;
        self.reload_view();
    }

    /// `CheckHistoryAdd`: a new selection after the current one cuts the
    /// forward part of the history.
    fn remember_history(&mut self, records: Vec<ViewRecord>) {
        if self.history.get(self.history_position) == Some(&records) {
            return;
        }
        self.history.truncate(self.history_position + 1);
        self.history.push(records);
        if self.history.len() > 50 {
            self.history.remove(0);
        }
        self.history_position = self.history.len() - 1;
    }

    /// `bnBackClick`/`bnForwardClick`.
    fn walk_history(&mut self, forward: bool) {
        let position = if forward {
            (self.history_position + 1).min(self.history.len().saturating_sub(1))
        } else {
            self.history_position.saturating_sub(1)
        };
        if position == self.history_position {
            return;
        }
        self.history_position = position;
        let records = self.history[position].clone();
        self.show_records(records);
    }

    /// The single-record overload of [`TfrmMain::set_active_records`].
    pub fn set_active_record(&mut self, record: ViewRecord) {
        self.set_active_records(vec![record]);
    }

    /// `vstNavSelectionChanged`: the selected main records fill the view
    /// (`GetSortedSelection`); a focused file node shows its header record
    /// (`_File.Elements[0]`), as the file rows did before the shared
    /// [`Selection`] landed. The view keeps no selection of its own.
    fn show_nav_selection(&mut self) {
        let mut records: Vec<ViewRecord> = self
            .selection
            .active_records()
            .into_iter()
            .filter_map(|node| {
                Some(ViewRecord {
                    form_id: node.form_id?,
                    file: node.file?,
                })
            })
            .collect();
        if records.is_empty()
            && let Some(node) = self
                .selection
                .focused
                .as_ref()
                .filter(|node| node.kind == NodeKind::File)
            && let Some(file) = node.file.as_ref()
            && let Some(index) = self.files.iter().position(|name| name == file)
            && let Some(id) = self.file_ids.get(index).copied().filter(|id| *id >= 0)
        {
            records.push(ViewRecord {
                form_id: format!("{id:02X}000000"),
                file: file.clone(),
            });
        }
        if records != self.view_selection {
            self.set_active_records(records);
        }
    }

    /// `DoSetActiveRecord`: builds the view from the command layer again and
    /// keeps the element that had the focus.
    pub fn reload_view(&mut self) {
        let selection = self.view_selection.clone();
        let (model, error) = self.build_view_model(&selection);
        if let Some(error) = error {
            self.add_message(&format!("Fatal: {error}"));
        }
        self.view.rebuild(model);
        self.update_column_widths();
        if self.active_tab != Tab::ReferencedBy {
            self.active_tab = Tab::View;
        }
    }

    /// `UpdateColumnWidths`: the record columns take the widths of the
    /// `mniViewColumnWidth*` mode (`Options.ColumnWidth` in the standard
    /// mode, the client width shared in the fit mode). The user's own
    /// resizes last until the next view or mode.
    fn update_column_widths(&mut self) {
        let widths = crate::view_grid::grid_columns(
            &self.view.model,
            self.settings.view.column_width,
            self.settings.window.width as f32,
        );
        if widths.len() == self.view.state.column_widths.len() {
            for (state, source) in self.view.state.column_widths.iter_mut().zip(&widths) {
                *state = source.width;
            }
        }
    }

    /// The `records.compare` request of the view: the record shown with its
    /// overrides, the view's options and the filter strip.
    fn compare_request(&self, selected: &ViewRecord) -> CompareRequest {
        CompareRequest {
            form_id: selected.form_id.clone(),
            file: Some(selected.file.clone()),
            master_and_leafs: self.view_master_and_leafs,
            hide_no_conflict: self.settings.view.hide_no_conflict,
            include_hidden: false,
            raw_data: false,
            mod_groups: Vec::new(),
            all_mod_groups: false,
            saved_mod_groups: self.view_mod_groups,
            view_filter_name: non_empty(&self.view_filter.name),
            view_filter_value: non_empty(&self.view_filter.value),
            view_filter_or: self.view_filter.or,
            keep_children: self.view_filter.keep_children,
            keep_siblings: self.view_filter.keep_siblings,
            keep_parents_siblings: self.view_filter.keep_parents_siblings,
        }
    }

    /// The sibling comparison request (`records.compare_selected`).
    fn compare_selected_request(&self, selection: &[ViewRecord]) -> CompareSelectedRequest {
        CompareSelectedRequest {
            records: selection
                .iter()
                .map(|record| SelectedRecord {
                    form_id: record.form_id.clone(),
                    file: Some(record.file.clone()),
                })
                .collect(),
            master_and_leafs: self.view_master_and_leafs,
            hide_no_conflict: self.settings.view.hide_no_conflict,
            include_hidden: false,
            raw_data: false,
            mod_groups: Vec::new(),
            all_mod_groups: false,
            saved_mod_groups: self.view_mod_groups,
            view_filter_name: non_empty(&self.view_filter.name),
            view_filter_value: non_empty(&self.view_filter.value),
            view_filter_or: self.view_filter.or,
            keep_children: self.view_filter.keep_children,
            keep_siblings: self.view_filter.keep_siblings,
            keep_parents_siblings: self.view_filter.keep_parents_siblings,
        }
    }

    /// Builds the view model through the command layer (design rule 1).
    fn build_view_model(&mut self, selection: &[ViewRecord]) -> (ViewModel, Option<String>) {
        if selection.is_empty() {
            return (ViewModel::empty(), None);
        }
        if selection.len() == 1 {
            let selected = selection[0].clone();
            let request = self.compare_request(&selected);
            let Some(bridge) = self.bridge.as_mut() else {
                return (ViewModel::empty(), None);
            };
            return match bridge.request::<_, CompareResponse>("records.compare", &request) {
                Ok(response) => (ViewModel::from_compare(&response, &selected), None),
                Err(error) => (ViewModel::empty(), Some(format!("records.compare: {}", error.message))),
            };
        }
        let request = self.compare_selected_request(selection);
        let Some(bridge) = self.bridge.as_mut() else {
            return (ViewModel::empty(), None);
        };
        match bridge.request::<_, CompareSelectedResponse>("records.compare_selected", &request) {
            Ok(response) => (ViewModel::from_compare_selected(&response), None),
            Err(error) => (
                ViewModel::empty(),
                Some(format!("records.compare_selected: {}", error.message)),
            ),
        }
    }

    /// `FormClose`: the window placement and the splitter back into the
    /// settings file.
    pub fn save_settings(&mut self, ctx: &egui::Context) {
        // `TForm.Width` is in physical pixels; egui reports logical points.
        let scale = ctx.pixels_per_point().max(0.5);
        if let Some(rect) = ctx.input(|input| input.viewport().inner_rect) {
            self.settings.window.width = (rect.width() * scale).round() as i32;
            self.settings.window.height = (rect.height() * scale).round() as i32;
        }
        if let Some(rect) = ctx.input(|input| input.viewport().outer_rect) {
            self.settings.window.left = (rect.left() * scale).round() as i32;
            self.settings.window.top = (rect.top() * scale).round() as i32;
        }
        self.settings.window.state = if ctx.input(|input| input.viewport().maximized.unwrap_or(false)) {
            WindowState::Maximized
        } else {
            WindowState::Normal
        };
        self.settings.nav_column_widths = self.nav_state.column_widths.clone();
        if let Err(error) = self.settings.save() {
            eprintln!("xedit-gui: could not write the settings file: {error}");
        }
    }

    /// Applies the theme and the fonts once the game is chosen.
    fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.theme_applied {
            return;
        }
        let visuals = match self.settings.theme {
            crate::theme::ThemeChoice::Light => egui::Visuals::light(),
            crate::theme::ThemeChoice::Dark => egui::Visuals::dark(),
            _ => {
                if ctx.global_style().visuals.dark_mode {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                }
            }
        };
        ctx.set_visuals(visuals);
        apply_font_and_scale(ctx, &self.settings.fonts);
        self.theme_applied = true;
    }

    /// The top bar (`pnlTop`).
    fn top_bar(&mut self, ui: &mut egui::Ui) -> Option<ChosenMenu> {
        let ctx = ui.ctx().clone();
        let mut command = None;
        egui::Panel::top("pnlTop").show(ui, |ui| {
            ui.horizontal(|ui| {
                // `bnMainMenu`: the hamburger opens `pmuMain`.
                ui.menu_button("\u{2630}", |ui| {
                    // `pmuMainPopup` fills the language list of the
                    // Localization submenu when the menu opens.
                    let runtime = self.runtime_menus();
                    command = draw_menu(
                        ui,
                        &menu_model::PMU_MAIN,
                        &|id| self.menu_item_enabled(id, false),
                        &runtime,
                    );
                })
                .response
                .on_hover_text("Main menu");
                // `bnBack` and `bnForward`: the selection history.
                let back = ui
                    .add_enabled(self.history_position > 0, egui::Button::new("\u{25C0}"))
                    .on_hover_text("Back");
                if back.clicked() {
                    self.walk_history(false);
                }
                let forward = ui
                    .add_enabled(
                        self.history_position + 1 < self.history.len(),
                        egui::Button::new("\u{25B6}"),
                    )
                    .on_hover_text("Forward");
                if forward.clicked() {
                    self.walk_history(true);
                }
                // `lblPath`: the path of the selected record, read only.
                let path_text = self.selected_path_text();
                let mut path = path_text.clone();
                let width = (ui.available_width() - 460.0).max(80.0);
                ui.add_enabled(
                    false,
                    egui::TextEdit::singleline(&mut path)
                        .desired_width(width)
                        .hint_text("No selection"),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // `pnlBtn`: the project links.
                    let strip = ui.horizontal(|ui| {
                        for (id, caption, url) in [
                            ("bnHelp", "Help", "https://tes5edit.github.io/docs"),
                            (
                                "bnVideos",
                                "Videos",
                                "https://www.youtube.com/playlist?list=PLlN8weLk86XiGXJI4DaRa1QIq1zhDpD8V",
                            ),
                            ("bnDiscord", "Discord", "https://discord.gg/5t8RnNQ"),
                            ("bnGitHub", "GitHub", "https://github.com/TES5Edit/TES5Edit/releases"),
                            ("bnNexusMods", "NexusMods", "https://www.nexusmods.com/"), // wbNexusModsUrl per game
                            ("bnKoFi", "Ko-Fi", "https://www.ko-fi.com/ElminsterAU"),
                            ("bnPatreon", "Patreon", "https://www.patreon.com/ElminsterAU"),
                            ("bnPayPal", "PayPal", "https://paypal.me/ElminsterAU"),
                        ] {
                            let caption = if self.shrink_buttons {
                                caption.chars().next().map(String::from).unwrap_or_default()
                            } else {
                                caption.to_owned()
                            };
                            let response = ui.small_button(caption);
                            if response.clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(url));
                            }
                            let _ = (id, response);
                        }
                    });
                    // `pmuBtnMenu`. The strip is a layout, so it has no
                    // click of its own: the click is read from the pointer
                    // state, as for the other containers.
                    menu_model::container_context_menu(&strip.response, |ui| {
                        if ui.checkbox(&mut self.shrink_buttons, "Shrink Buttons").changed() {
                            // `mniBtnShrinkButtonsClick` writes the setting.
                            self.write_settings(
                                "Options",
                                "ShrinkButtons",
                                serde_json::Value::Bool(self.shrink_buttons),
                                "bool",
                            );
                            ui.close();
                        }
                        ui.separator();
                        if let Some(theme) = vcl_styles_system_menu(ui, &self.settings.theme.clone()) {
                            self.settings.theme = theme;
                            self.settings.theme.apply(&ctx);
                            self.theme_applied = true;
                        }
                    });
                });
            });
        });
        command
    }

    /// The `lblPath` text: the focused node and its ancestors
    /// (`vstNavChange`).
    fn selected_path_text(&self) -> String {
        if self.is_pinned() {
            return self.pinned_path.clone();
        }
        self.nav.path_text(&self.nav_state)
    }

    /// `IsPinned`: `bnPinned.Down and (LockOutPinnedCount < 1)`. While the
    /// pin is on, the navigation tree moves without the view and the path
    /// label following it.
    fn is_pinned(&self) -> bool {
        self.pinned && self.pin_lockout == 0
    }

    /// `bnPinnedClick`: the button keeps the view on the record it shows
    /// while the tree moves on; unpinning runs the nav change of the
    /// focused node again, which updates the path and the view.
    fn toggle_pin(&mut self) {
        self.pinned = !self.pinned;
        if self.pinned {
            self.pinned_path = self.nav.path_text(&self.nav_state);
            return;
        }
        self.pinned_path.clear();
        // `vstNavChange(vstNav, vstNav.FocusedNode)`: the label and the
        // view follow the focused node again.
        self.show_nav_selection();
    }

    /// `pnlNav`: `pnlSearch` at the top, `lblFilterHint` under it when a
    /// filter is applied, `pnlNavBottom` at the bottom and `vstNav` between
    /// (the DFM align order).
    fn nav_panel(&mut self, ui: &mut egui::Ui) -> Vec<NavAction> {
        let mut actions = Vec::new();
        let panel = egui::Panel::left("pnlNav")
            .resizable(true)
            .default_size(self.settings.nav_width)
            .min_size(50.0)
            .show(ui, |ui| {
                let tree_id = ui.scope_id().with("vstNav");
                egui::Panel::top("pnlNavSearch").show(ui, |ui| {
                    self.nav_search_panel(ui);
                });
                egui::Panel::bottom("pnlNavBottom").show(ui, |ui| {
                    self.nav_bottom_panel(ui, tree_id);
                });
                if self.nav.filter.applied {
                    // `lblFilterHint`.
                    ui.add(egui::Label::new(FILTER_HINT).wrap());
                }
                let add_list = self.nav.add_list(&self.nav_state);
                let hidden = crate::nav_tree::hidden_checked(&self.nav, &self.nav_state);
                let kind = self.nav.focused_kind(&self.nav_state);
                self.nav.prepare(kind);
                let output = TreeView {
                    id: tree_id,
                    source: &mut self.nav,
                    state: &mut self.nav_state,
                    colors: &self.settings.colors,
                    header: true,
                }
                .show(ui);
                let response = ui.response();
                // The tree has just handled the click: a right click focuses
                // the node it was made on (`vstNav` sets its focused node
                // before the popup menu shows), so the menu reads the
                // selection again.
                let selection = self.nav.selection(&self.nav_state);
                menu_model::container_context_menu(&response, |ui| {
                    actions.extend(draw_nav_menu(
                        ui,
                        &menu_model::PMU_NAV,
                        &selection,
                        hidden,
                        self.edit_allowed,
                        &add_list,
                        self.mod_groups_present,
                    ));
                });
                // `Header.PopupMenu = pmuNavHeaderPopup`.
                if let Some(header) = output.header_response {
                    menu_model::context_menu(&header, |ui| {
                        actions.extend(draw_nav_menu(
                            ui,
                            &menu_model::PMU_NAV_HEADER_POPUP,
                            &selection,
                            hidden,
                            self.edit_allowed,
                            &add_list,
                            self.mod_groups_present,
                        ));
                    });
                }
            });
        self.settings.nav_width = panel.response.rect.width();
        actions
    }

    /// `pnlSearch`: `edFormIDSearch` and `edEditorIDSearch`.
    fn nav_search_panel(&mut self, ui: &mut egui::Ui) {
        let dark = ui.visuals().dark_mode;
        ui.horizontal(|ui| {
            ui.label("FormID");
            let mut edit = egui::TextEdit::singleline(&mut self.nav.search.form_id).desired_width(80.0);
            if let Some(color) = self.nav.search.form_id_state.color(dark) {
                edit = edit.background_color(color);
            }
            let response = ui.add(edit);
            if response.changed() {
                // `edFormIDSearchChange`: the box turns back to `clWindow`.
                self.nav.search.form_id_state = SearchState::Normal;
            }
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.run_form_id_search();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Editor ID");
            let width = (ui.available_width() - 70.0).max(60.0);
            let mut edit = egui::TextEdit::singleline(&mut self.nav.search.editor_id).desired_width(width);
            if let Some(color) = self.nav.search.editor_id_state.color(dark) {
                edit = edit.background_color(color);
            }
            let response = ui.add(edit);
            if response.changed() {
                self.nav.search.editor_id_state = SearchState::Normal;
            }
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.run_editor_id_search();
            }
        });
    }

    /// `pnlNavBottom`: `edFileNameFilter` and `cbRegExFilter`.
    fn nav_bottom_panel(&mut self, ui: &mut egui::Ui, tree_id: egui::Id) {
        let dark = ui.visuals().dark_mode;
        let mut changed = false;
        let mut focus_tree = false;
        ui.horizontal(|ui| {
            ui.label("Filter by filename:");
            let width = (ui.available_width() - 70.0).max(60.0);
            let mut edit = egui::TextEdit::singleline(&mut self.nav.search.file_name).desired_width(width);
            if let Some(color) = self.nav.search.file_state.color(dark) {
                edit = edit.background_color(color);
            }
            let response = ui.add(edit);
            if response.changed() {
                changed = true;
            }
            if response.lost_focus()
                && ui.input(|input| input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::ArrowUp))
            {
                // `edFileNameFilterKeyDown`.
                focus_tree = true;
            }
            if ui.checkbox(&mut self.nav.search.file_regex, "RegEx").changed() {
                // `cbRegExFilterClick`: the text is read again.
                self.nav.search.file_state = SearchState::Normal;
                changed = true;
            }
        });
        if changed {
            self.run_file_name_filter();
        }
        if focus_tree {
            ui.memory_mut(|memory| memory.request_focus(tree_id));
            if self.nav_state.focused.is_none_or(|row| row >= self.nav.row_count()) {
                self.nav_state.select_only(0);
            }
        }
    }

    /// `edFileNameFilterChange`: `files.list` with the text as `name_filter`.
    fn run_file_name_filter(&mut self) {
        let text = self.nav.search.file_name.clone();
        if text.is_empty() {
            self.nav.set_file_visibility(Vec::new());
            self.nav.search.file_state = SearchState::Normal;
            return;
        }
        let regex = self.nav.search.file_regex;
        match self.call_session("files.list", json!({ "name_filter": text, "regex": regex })) {
            Ok(value) => {
                let names: Vec<String> = value["files"]
                    .as_array()
                    .map(|files| {
                        files
                            .iter()
                            .filter_map(|file| file["name"].as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                self.nav.set_file_visibility(names);
                self.nav.search.file_state = SearchState::Normal;
            }
            // `ERegularExpressionError` while typing: the box turns red and
            // the previous visibility stays.
            Err(_) => self.nav.search.file_state = SearchState::NotFound,
        }
    }

    /// `edFormIDSearchKeyDown`: `records.get` for the FormID, then the node
    /// of the record is revealed and selected.
    fn run_form_id_search(&mut self) {
        let text = self.nav.search.form_id.trim().to_owned();
        if text.is_empty() {
            self.nav.search.form_id_state = SearchState::NotFound;
            return;
        }
        let mut text = text;
        if self.settings.options.convert_int_form_id && !text.starts_with('0') && !text.starts_with("0x") {
            text = match text.parse::<u32>() {
                Ok(value) => format!("{value:08X}"),
                Err(_) => "00000000".to_owned(),
            };
        }
        if let Some(rest) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            text = rest.to_owned();
        }
        let typed = format!("{:0>8}", text.to_uppercase());
        match self.call_session("records.get", json!({ "form_id": typed, "depth": 0 })) {
            Ok(value) => {
                let file = value["summary"]["file"].as_str().unwrap_or_default().to_owned();
                let form_id = value["summary"]["form_id"].as_str().unwrap_or(&typed).to_owned();
                if let Some(node) = self.nav.find_record(&form_id, Some(&file))
                    && let Some(row) = self.nav.reveal(node)
                {
                    self.nav_state.select_only(row);
                    self.nav_state.scroll_to = Some(row);
                    self.nav.search.form_id_state = SearchState::Found;
                    return;
                }
                // `JumpTo(MainRecord, False)` shows the record in the view;
                // without a node the box turns yellow.
                self.nav.search.form_id_state = SearchState::NotInTree;
            }
            Err(_) => self.nav.search.form_id_state = SearchState::NotFound,
        }
    }

    /// `edEditorIDSearchKeyDown`: `records.find` for the editor ID, starting
    /// after the focused node, and the first matching node is revealed.
    ///
    /// UPSTREAM-QUIRK: the upstream loop walks the initialised nodes only,
    /// so a record inside a collapsed group is not found either.
    fn run_editor_id_search(&mut self) {
        let text = self.nav.search.editor_id.trim().to_owned();
        if text.is_empty() {
            self.nav.search.editor_id_state = SearchState::NotFound;
            return;
        }
        let lower = text.to_lowercase();
        let focused_row = self.nav_state.focused;
        match self.call_session("records.find", json!({ "editor_id": text, "limit": 1000 })) {
            Ok(value) => {
                let mut chosen = None;
                for record in value["records"].as_array().into_iter().flatten() {
                    let editor_id = record["editor_id"].as_str().unwrap_or_default().to_lowercase();
                    if !editor_id.starts_with(&lower) {
                        continue;
                    }
                    let form_id = record["form_id"].as_str().unwrap_or_default();
                    let file = record["file"].as_str().unwrap_or_default();
                    let Some(node) = self.nav.find_record(form_id, Some(file)) else {
                        continue;
                    };
                    let Some(row) = self.nav.reveal(node) else {
                        continue;
                    };
                    if chosen.is_none() {
                        chosen = Some(row);
                    }
                    if focused_row.is_none_or(|focused| row > focused) {
                        chosen = Some(row);
                        break;
                    }
                }
                match chosen {
                    Some(row) => {
                        self.nav_state.select_only(row);
                        self.nav_state.scroll_to = Some(row);
                        self.nav.search.editor_id_state = SearchState::Found;
                    }
                    None => self.nav.search.editor_id_state = SearchState::NotInTree,
                }
            }
            Err(_) => self.nav.search.editor_id_state = SearchState::NotFound,
        }
    }

    /// The view a navigation tree row shows: the file header of a file row
    /// (`Supports(aElement, IwbFile) and _File.Elements[0]`).
    pub fn activate_nav_file(&mut self, index: usize) {
        let Some(file) = self.files.get(index).cloned() else {
            return;
        };
        // The header record's load order FormID is the file's load order
        // file id in the high byte (`TwbFile.Elements[0]`). The hardcoded
        // game executable has no slot, so its header cannot be addressed.
        let Some(id) = self.file_ids.get(index).copied().filter(|id| *id >= 0) else {
            self.add_message(&format!(
                "{file} is the hardcoded game file: its header has no load order FormID"
            ));
            return;
        };
        let form_id = format!("{id:02X}000000");
        self.set_active_record(ViewRecord {
            form_id: form_id.clone(),
            file: file.clone(),
        });
        self.set_pane_record(Some(RecordSelection::new(form_id, Some(file))));
    }

    /// `pgMain` with the tab strip at the bottom (`TabPosition = tpBottom`).
    fn view_area(&mut self, ui: &mut egui::Ui) -> (Option<ChosenMenu>, PaneAction) {
        let mut command = None;
        let mut action = PaneAction::None;
        let visible = self.visible_tabs();
        if !visible.contains(&self.active_tab) {
            self.active_tab = if visible.contains(&Tab::View) {
                Tab::View
            } else {
                Tab::Messages
            };
        }
        let _tabs = egui::Panel::bottom("pgMainTabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                for tab in &visible {
                    let caption = match tab {
                        Tab::ReferencedBy => self.referenced_by.caption(self.active_tab == *tab),
                        Tab::Messages if self.messages.highlighted => format!("{} *", tab.caption()),
                        _ => tab.caption().to_owned(),
                    };
                    if ui.selectable_label(self.active_tab == *tab, caption).clicked() {
                        self.active_tab = *tab;
                        if *tab == Tab::Messages {
                            self.messages.highlighted = false;
                        }
                    }
                }
            });
        });
        #[cfg(test)]
        self.rects.push(("pgMainTabs", _tabs.response.rect));
        if self.active_tab == Tab::ReferencedBy && self.referenced_by.loaded_for.is_none() {
            self.referenced_by.reload = true;
        }
        // The element detail editor of `vstViewEditing` and
        // `mniViewEditClick`, tied to the grid selection. It is a side panel
        // of the tab area, so it takes its space from the central panel
        // instead of painting over it.
        if self.active_tab == Tab::View
            && let Some(bridge) = self.bridge.as_mut()
        {
            egui::Panel::right("xeElementDetail")
                .resizable(true)
                .default_size(360.0)
                .min_size(180.0)
                .show(ui, |ui| {
                    self.element_detail.show(ui, bridge, self.edit_allowed);
                });
        }
        let _central = egui::CentralPanel::default_margins().show(ui, |ui| {
            if self.bridge.is_none() {
                ui.label("No session is loaded.");
                return;
            }
            match self.active_tab {
                Tab::View => {
                    command = self.draw_view(ui);
                }
                Tab::ReferencedBy => {
                    let Some(bridge) = self.bridge.as_mut() else {
                        return;
                    };
                    action = panes::referenced_by_pane(
                        ui,
                        &mut self.referenced_by,
                        bridge,
                        self.active_record.as_ref(),
                        &self.files,
                        self.edit_allowed,
                        &self.settings.colors,
                    );
                }
                Tab::Messages => {
                    let font = crate::theme::messages_font_id(&self.settings.fonts);
                    let id = ui.scope_id().with("mmoMessages");
                    action = self.messages.show(ui, font, id);
                }
                Tab::Information => {
                    let font = crate::theme::information_font_id();
                    let id = ui.scope_id().with("Memo1");
                    self.information_selection = panes::information_pane(ui, font, id);
                    // `mniViewEditClick`'s localization branch: the strings of
                    // the selected record, read on first show.
                    if let Some(record) = self.active_record.clone() {
                        if self.localization.loaded_for.as_ref() != Some(&record)
                            && let Some(bridge) = self.bridge.as_mut()
                        {
                            let _ = self.localization.load(bridge, &record);
                        }
                        self.localization.show(ui);
                    }
                }
                Tab::Weapon | Tab::Armor | Tab::Ammunition => {
                    let index = match self.active_tab {
                        Tab::Weapon => 0,
                        Tab::Armor => 1,
                        _ => 2,
                    };
                    let Some(bridge) = self.bridge.as_mut() else {
                        return;
                    };
                    action = panes::spreadsheet_pane(
                        ui,
                        &mut self.spreadsheets[index],
                        bridge,
                        &self.files,
                        self.game.is_some_and(spreadsheets_for_game),
                        &self.settings.colors,
                        self.edit_allowed,
                    );
                }
            }
        });
        #[cfg(test)]
        self.rects.push(("central", _central.response.rect));
        (command, action)
    }

    /// The tabs `pgMain` shows for the current state: the spreadsheets only
    /// for the games `WMUserLoaderDone` shows them for, Referenced By while a
    /// record with its references is chosen (`Tag > 0`).
    fn visible_tabs(&self) -> Vec<Tab> {
        let spreadsheets = self.game.is_some_and(spreadsheets_for_game);
        Tab::ALL
            .into_iter()
            .filter(|tab| match tab {
                Tab::ReferencedBy => self.referenced_by.visible() || self.active_record.is_some(),
                Tab::Weapon | Tab::Armor | Tab::Ammunition => spreadsheets,
                _ => true,
            })
            .collect()
    }

    /// Runs the action a pane asked for.
    fn run_pane_action(&mut self, ctx: &egui::Context, action: PaneAction) {
        match action {
            PaneAction::None => {}
            PaneAction::Jump(record) => {
                self.set_pane_record(Some(record.clone()));
                // `JumpTo`: the navigation tree shows the record too.
                self.reveal_in_navigation_tree(&record.form_id, record.file.as_deref());
            }
            PaneAction::ShowView => self.active_tab = Tab::View,
            PaneAction::Messages(lines) => {
                for line in lines {
                    self.add_message(&line);
                }
            }
            PaneAction::Copy(text) => ctx.copy_text(text),
        }
    }

    /// `JumpTo`: the navigation tree reveals the record and selects it.
    fn reveal_in_navigation_tree(&mut self, form_id: &str, file: Option<&str>) {
        if let Some(node) = self.nav.find_record(form_id, file)
            && let Some(row) = self.nav.reveal(node)
        {
            self.nav_state.select_only(row);
            self.nav_state.scroll_to = Some(row);
        }
    }

    /// `mniViewHeaderJumpToClick`: the record of the view's header row goes
    /// into the navigation tree.
    fn jump_to_navigation_tree(&mut self) {
        let Some(record) = self.header_view_record().cloned() else {
            self.add_message("No record is selected.");
            return;
        };
        self.reveal_in_navigation_tree(&record.form_id, Some(record.file.as_str()));
    }

    /// The popup rules of `pmuViewPopup`: whether a menu item applies to the
    /// focused view cell. An item that does not is drawn disabled rather
    /// than hidden, so the whole upstream command surface stays visible.
    fn menu_item_enabled(&self, id: &'static str, is_header: bool) -> bool {
        // `pmuViewPopup` leaves every edit item hidden without `wbEditAllowed`.
        if !self.edit_allowed
            && matches!(
                id,
                "mniViewEdit"
                    | "mniViewSetToDefault"
                    | "mniViewAdd"
                    | "mniViewRemove"
                    | "mniViewClear"
                    | "mniViewNextMember"
                    | "mniViewPreviousMember"
                    | "mniViewMoveUp"
                    | "mniViewMoveDown"
                    | "mniViewRemoveFromSelected"
                    | "mniViewCopyToSelectedRecords"
                    | "mniViewCopyMultipleToSelectedRecords"
                    | "mniViewHeaderCopyAsOverride"
                    | "mniViewHeaderCopyAsOverrideWithOverwriting"
                    | "mniViewHeaderDeepCopyAsOverride"
                    | "mniViewHeaderDeepCopyAsOverrideWithOverwriting"
                    | "mniViewHeaderCopyAsNewRecord"
                    | "mniViewHeaderCopyAsWrapper"
                    | "mniViewHeaderRemove"
            )
        {
            return false;
        }
        let cell = self.focused_view_cell();
        let element = cell.is_some();
        let editable = cell.is_some_and(|(cell, _)| cell.editable);
        let sibling = self.view.model.kind == ViewKind::Multiple;
        // The header items act on the column the header menu was opened on;
        // they apply while the view has a record column.
        let header = is_header && !self.view.model.columns.is_empty();
        match id {
            "mniViewEdit" | "mniViewSetToDefault" => editable,
            "mniViewRemove"
            | "mniViewClear"
            | "mniViewNextMember"
            | "mniViewPreviousMember"
            | "mniViewMoveUp"
            | "mniViewMoveDown"
            | "mniViewCompareReferencedRow" => element,
            "mniViewAdd" => self.view_add_target().is_some(),
            "mniViewRemoveFromSelected" | "mniViewCopyToSelectedRecords" => sibling && element,
            "mniViewCopyMultipleToSelectedRecords" => sibling,
            "mniViewSort" => sibling,
            "mniViewHeaderCopyAsOverride"
            | "mniViewHeaderCopyAsOverrideWithOverwriting"
            | "mniViewHeaderDeepCopyAsOverride"
            | "mniViewHeaderDeepCopyAsOverrideWithOverwriting"
            | "mniViewHeaderCopyAsNewRecord"
            | "mniViewHeaderCopyAsWrapper"
            | "mniViewHeaderRemove"
            | "mniViewHeaderJumpTo"
            | "mniViewHeaderHidden"
            | "mniViewHeaderUnhideAll"
            | "mniViewCreateModGroup" => header,
            "mniCopyPathToClipboard"
            | "mniCopyFullPathToClipboard"
            | "mniCopyIndexedPathToClipBoard"
            | "mniCopyPathNameToClipboard"
            | "mniCopySignatureToClipboard"
            | "mniCopyNameToClipboard"
            | "mniCopyDisplayNameToClipboard"
            | "mniCopyShortNameToClipboard" => element,
            // `pmuMainPopup` shows the Localization menu of Skyrim,
            // Fallout 4, Fallout 76 and Starfield only; the port draws the
            // item disabled rather than hidden.
            "mniMainLocalizationLanguage" => self.localization_languages_offered(),
            _ => true,
        }
    }

    /// Whether the game has the string tables the language list is made
    /// of: Skyrim, Fallout 4, Fallout 76 and Starfield
    /// (`pmuMainPopup`'s `wbIsSkyrim or wbIsFallout4 or wbIsFallout76 or
    /// wbIsStarfield`).
    fn localization_languages_offered(&self) -> bool {
        matches!(
            self.game,
            Some(
                GameMode::gmTES5
                    | GameMode::gmSSE
                    | GameMode::gmEnderal
                    | GameMode::gmEnderalSE
                    | GameMode::gmTES5VR
                    | GameMode::gmFO4
                    | GameMode::gmFO4VR
                    | GameMode::gmFO76
                    | GameMode::gmSF1,
            )
        )
    }

    /// The items of the menus upstream builds at run time. The language
    /// list is asked for once per session and after every switch, not on
    /// every frame of the open menu.
    fn runtime_menus(&mut self) -> Vec<menu_model::RuntimeMenu> {
        if !self.language_menu_loaded {
            self.refresh_language_menu();
        }
        self.language_menu.clone()
    }

    /// `pmuMainPopup`: the languages of the session, one radio item each,
    /// the language of the session checked (`AvailableLanguages`).
    fn refresh_language_menu(&mut self) {
        self.language_menu_loaded = true;
        let mut items = Vec::new();
        if let Some(bridge) = self.bridge.as_mut()
            && let Ok(response) = bridge.request::<_, xedit_session::localization::LocalizationLanguagesResponse>(
                "localization.languages",
                &xedit_session::NoParams {},
            )
        {
            for language in &response.available {
                items.push(menu_model::RuntimeItem {
                    caption: language.clone(),
                    command: "localization.language",
                    // `SameText(sl[i], wbLanguage)`.
                    checked: language.eq_ignore_ascii_case(&response.language),
                    param: language.clone(),
                });
            }
        }
        self.language_menu = vec![menu_model::RuntimeMenu {
            id: "mniMainLocalizationLanguage",
            items,
        }];
    }

    /// `mniMainLocalizationLanguageClick`: the language of the string
    /// tables changes, the tables are read again and the tree and the view
    /// are invalidated (`vstNav.Invalidate`, `vstView.Invalidate`).
    fn switch_language(&mut self, language: &str) {
        let request = xedit_session::localization::LocalizationLanguageRequest {
            language: language.to_owned(),
            // Upstream drops the changed tables without asking
            // (`wbLocalizationHandler.Clear`).
            discard: true,
        };
        let Some(response) = self
            .call::<_, xedit_session::localization::LocalizationLanguageResponse>("localization.language", &request)
        else {
            return;
        };
        if response.changed {
            self.add_message(&format!("Language: {}", response.language));
        }
        self.nav.refresh();
        self.reload_view();
        self.refresh_language_menu();
    }

    /// The data column (0-based) the view's focus is on
    /// (`Pred(FocusedColumn)` of `vstView`), when it is a record column.
    fn focused_view_column(&self) -> Option<usize> {
        let column = self.view.state.focused_column;
        (column > 0 && column <= self.view.model.columns.len()).then(|| column - 1)
    }

    /// The focused view cell and its data column.
    fn focused_view_cell(&self) -> Option<(&crate::view_grid::ViewCell, usize)> {
        let row = self.view.state.focused_row?;
        let column = self.focused_view_column()?;
        let cell = self.view.model.cell(row, column)?;
        cell.path.as_ref()?;
        Some((cell, column))
    }

    /// The record of the focused data column.
    fn focused_view_record(&self) -> Option<&ViewRecord> {
        self.view.model.records.get(self.focused_view_column()?)
    }

    /// `vstViewDblClick`: a double click on the label column toggles the
    /// row (or sorts the columns, when the view compares siblings), and a
    /// double click on a data column either starts the in-place editor (an
    /// integer, a flag or a float) or opens the View Elements dialog with a
    /// page per record.
    fn view_double_click(&mut self, row: usize, column: usize, shift: bool) {
        if column == 0 {
            // `vstViewDblClick` with the label column: a sibling comparison
            // sorts the columns, every other view folds the row.
            if self.view.model.kind == ViewKind::Multiple {
                self.view.state.focused_row = Some(row);
                self.view_sort();
                return;
            }
            self.view.model.toggle(row);
            return;
        }
        let Some(data_column) = column.checked_sub(1) else {
            return;
        };
        let Some(cell) = self.view.model.cell(row, data_column) else {
            return;
        };
        let Some(path) = cell.path.clone() else {
            return;
        };
        let editable = cell.editable;
        let edit_value = cell.edit_value.clone();
        let value = cell.value.clone();
        // `vstViewDblClick`: the in-place editor takes an integer, a flag or
        // a float; every other type opens the View Elements dialog. The
        // type is the one `elements.get` reports for the element.
        let kind = self.view_element_kind(data_column, &path);
        if editable && kind.is_some_and(|kind| kind.edits_in_place()) {
            let grid_cell = crate::widgets::grid::GridCell {
                text: value.unwrap_or_default(),
                tooltip: None,
                editable: true,
                edit_text: edit_value,
                conflict_all: xedit_core::interface::types::ConflictAll::caUnknown,
                conflict_this: xedit_core::interface::types::ConflictThis::ctUnknown,
                cell_fill: None,
            };
            self.view.state.focused_row = Some(row);
            self.view.state.focused_column = column;
            self.view.state.on_double_click(row, column, &grid_cell);
            return;
        }
        self.open_view_elements(row, data_column, &path, shift);
    }

    /// The value type of the element of the row and data column
    /// (`elements.get`, then the type the value carries).
    fn view_element_kind(&mut self, data_column: usize, path: &str) -> Option<crate::element_detail::ValueType> {
        let record = self.view.model.records.get(data_column)?;
        let request = ElementsGetRequest {
            form_id: record.form_id.clone(),
            path: path.to_owned(),
            file: Some(record.file.clone()),
            depth: Some(0),
        };
        let detail = self.call::<_, ElementDetail>("elements.get", &request)?;
        Some(crate::element_detail::value_type(&detail.node))
    }

    /// `TfrmViewElements` of `vstViewDblClick`: a page per record of the
    /// view, each with the element of the row it holds. The pages are read
    /// with `elements.get`; the modal edit is allowed with
    /// `-IKnowWhatImDoing` or with the Shift key held, as upstream does.
    fn open_view_elements(&mut self, row: usize, data_column: usize, path: &str, shift: bool) {
        let records = self.view.model.records.clone();
        if data_column >= records.len() {
            return;
        }
        let modal_edit = xedit_core::interface::globals::i_know_what_im_doing() || shift;
        let mut state = ViewElementsState::default();
        if let Some(bridge) = self.bridge.as_mut() {
            state.compare_cmdline = setting_get_string(bridge, "External", "CompareCommandLine", "bcompare.exe %1 %2");
            state.placement.left = setting_get_integer(bridge, "frmViewElements", "Left", 0) as i32;
            state.placement.top = setting_get_integer(bridge, "frmViewElements", "Top", 0) as i32;
            state.placement.width = setting_get_integer(bridge, "frmViewElements", "Width", 565).max(200) as i32;
            state.placement.height = setting_get_integer(bridge, "frmViewElements", "Height", 325).max(150) as i32;
            state.placement_loaded = true;
        }
        for (index, record) in records.iter().enumerate() {
            let Some(cell) = self.view.model.cell(row, index) else {
                continue;
            };
            let Some(cell_path) = cell.path.clone() else {
                continue;
            };
            let cell_editable = cell.editable;
            let cell_edit_value = cell.edit_value.clone();
            // `AddElement` opens a page for the element of the record; the
            // element of the record is the one of the row it holds.
            let element_path = if index == data_column {
                path.to_owned()
            } else {
                cell_path
            };
            let request = ElementsGetRequest {
                form_id: record.form_id.clone(),
                path: element_path,
                file: Some(record.file.clone()),
                depth: Some(0),
            };
            let Some(detail) = self.call::<_, ElementDetail>("elements.get", &request) else {
                continue;
            };
            let editable = modal_edit && cell_editable && self.edit_allowed;
            let mut tab = ElementTab::from_cell(
                record.file.clone(),
                detail.node.value.clone(),
                cell_edit_value,
                editable,
                record.form_id.clone(),
                detail.path.clone(),
            );
            if index == data_column {
                tab.path = path.to_owned();
                state.active = state.tabs.len();
            }
            state.tabs.push(tab);
        }
        if state.tabs.is_empty() {
            self.add_message("No element is selected.");
            return;
        }
        self.dialogs.view_elements = Some(state);
    }

    /// The header column `pmuViewHeader` was opened on (`PopupIndex`), as a
    /// data column.
    fn header_view_column(&self) -> Option<usize> {
        let column = self.view_header_column?;
        (column > 0 && column <= self.view.model.columns.len()).then(|| column - 1)
    }

    /// The record of the header column `pmuViewHeader` was opened on.
    fn header_view_record(&self) -> Option<&ViewRecord> {
        self.view.model.records.get(self.header_view_column()?)
    }

    /// Runs a session command and reports its error in the message log.
    fn call_session_command(&mut self, name: &str, params: serde_json::Value) -> Option<serde_json::Value> {
        let bridge = self.bridge.as_mut()?;
        match bridge.call(name, params) {
            Ok(value) => Some(value),
            Err(error) => {
                self.add_message(&format!("Fatal: {}", error.message));
                self.view.error = Some(error.message);
                None
            }
        }
    }

    /// The `pnlViewTop` strip: `bnPinned`, the filter texts and the keep
    /// boxes (`fpnlViewFilter`). A change applies the filter by building the
    /// view again (`ApplyViewFilter` runs in the command layer).
    fn view_filter_strip(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        // `fpnlViewFilter` is a flow panel upstream: the filter controls wrap
        // onto a second row when the client area is too narrow.
        ui.horizontal_wrapped(|ui| {
            // `bnPinned`: a speed button that keeps the view on the record
            // it shows while the navigation tree moves on.
            let pinned = ui.selectable_label(self.pinned, "\u{1F4CC}");
            let pinned_clicked = pinned.clicked();
            pinned.on_hover_text("bnPinned: keep the view on this record while the tree moves on");
            if pinned_clicked {
                self.toggle_pin();
            }
            ui.label("Filter by Name:");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut self.view_filter.name).desired_width(120.0))
                .changed();
            egui::ComboBox::from_id_salt("cobViewFilter")
                .selected_text(if self.view_filter.or { "or" } else { "and" })
                .width(50.0)
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut self.view_filter.or, false, "and").changed();
                    changed |= ui.selectable_value(&mut self.view_filter.or, true, "or").changed();
                });
            ui.label("by Value:");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut self.view_filter.value).desired_width(120.0))
                .changed();
            ui.label("Keep");
            changed |= ui.checkbox(&mut self.view_filter.keep_children, "children").changed();
            changed |= ui.checkbox(&mut self.view_filter.keep_siblings, "siblings").changed();
            changed |= ui
                .checkbox(&mut self.view_filter.keep_parents_siblings, "parent's siblings")
                .changed();
        });
        if changed {
            self.reload_view();
        }
    }

    /// `vstViewHeaderClick`: a left click on the label column sets every
    /// record column to the same width (with Shift) or spreads them over the
    /// client width.
    fn view_header_click(&mut self, column: usize, shift: bool) {
        let count = self.view.model.grid_column_count();
        if column != 0 || count < 3 {
            return;
        }
        let label = self
            .view
            .state
            .column_widths
            .first()
            .copied()
            .unwrap_or(crate::view_grid::COLUMN_WIDTH);
        if shift {
            self.view.state.column_widths.fill(label);
        } else {
            let client = self.settings.window.width as f32;
            let per = ((client - label) / (count - 2) as f32).max(60.0);
            self.view.state.column_widths[1..count - 1].fill(per);
        }
    }

    /// The `tbsView` page: the filter strip, `vstView` and its context menus
    /// (`pmuView` on a cell, `pmuViewHeader` on the header).
    fn draw_view(&mut self, ui: &mut egui::Ui) -> Option<ChosenMenu> {
        self.view_filter_strip(ui);
        // The menu items the focused cell makes applicable (`pmuViewPopup`).
        let mut enabled_cell = std::collections::HashSet::new();
        collect_menu_ids(menu_model::PMU_VIEW.items, &mut |id| {
            if self.menu_item_enabled(id, false) {
                enabled_cell.insert(id);
            }
        });
        let mut enabled_header = std::collections::HashSet::new();
        collect_menu_ids(menu_model::PMU_VIEW_HEADER.items, &mut |id| {
            if self.menu_item_enabled(id, true) {
                enabled_header.insert(id);
            }
        });
        let mut chosen: Option<ChosenMenu> = None;
        let available = ui.available_width();
        let mut menu = |ui: &mut egui::Ui, is_header: bool, column: usize| {
            let (tree, enabled) = if is_header {
                (&menu_model::PMU_VIEW_HEADER, &enabled_header)
            } else {
                (&menu_model::PMU_VIEW, &enabled_cell)
            };
            let picked = draw_menu(ui, tree, &|id| enabled.contains(&id), &[]);
            if picked.is_some() {
                chosen = picked;
            }
            let _ = column;
        };
        let mut source = ViewSource::new(
            &mut self.view.model,
            self.view.state.focused_column,
            self.edit_allowed,
            self.settings.view.column_width,
            available,
        );
        let output = Grid {
            id: ui.scope_id().with("vstView"),
            source: &mut source,
            state: &mut self.view.state,
            colors: &self.settings.colors,
            header: true,
            context_menu: Some(&mut menu),
            own_double_click: false,
        }
        .show(ui);
        let pending = source.pending_edit.take();
        if let Some((column, shift)) = output.header_clicked {
            self.view_header_click(column, shift);
        }
        if let Some(column) = output.header_secondary_clicked {
            self.view_header_column = Some(column);
        } else if output.secondary_clicked.is_some() {
            self.view_header_column = None;
        }
        if let Some((row, column)) = output.double_clicked {
            // `ModalEdit := wbIKnowWhatImDoing or (GetKeyState(VK_SHIFT) < 0)`.
            let shift = ui.input(|input| input.modifiers.shift);
            self.view_double_click(row, column, shift);
        }
        self.view.remember_focus();
        if let Some(edit) = pending {
            self.apply_view_edit(edit);
        }
        if output.edit == Some(crate::widgets::grid::GridEditResult::Rejected) {
            self.add_message("The cell can not be edited in place");
        }
        let ctx = ui.ctx().clone();
        self.draw_view_dialogs(&ctx);
        chosen
    }

    /// `vstViewNewText`: the edit the grid accepted goes to `elements.set`
    /// with the element path the cell names; the view is built again after
    /// it (`PostResetActiveTree`, `InvalidateElementsTreeView`).
    fn apply_view_edit(&mut self, edit: PendingEdit) {
        let request = ElementsSetRequest {
            form_id: edit.form_id,
            path: edit.path,
            file: Some(edit.file),
            value: serde_json::Value::String(edit.value),
            dry_run: false,
        };
        let result = {
            let Some(bridge) = self.bridge.as_mut() else {
                return;
            };
            bridge.request::<_, ElementsSetResponse>("elements.set", &request)
        };
        match result {
            Ok(response) => {
                if !response.changed {
                    self.add_message(&format!("{} is unchanged", response.path));
                }
                self.reload_view();
            }
            Err(error) => {
                self.add_message(&format!("Fatal: {}", error.message));
                self.view.error = Some(error.message);
            }
        }
    }

    /// The small modal dialogs of the view: the target file of a copy
    /// (`xeFileSelectForm`, step 5), the delete confirmation and the mod
    /// group name (`xeModGroupEditForm`, step 5).
    fn draw_view_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut dialog) = self.copy_into.take() {
            let files = self.files.clone();
            let mut copy = false;
            let mut cancel = false;
            egui::Window::new("Copy into")
                .id(egui::Id::unique("viewCopyInto"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Copy {} into:",
                        crate::view_grid::ViewModel::record_label(&dialog.record)
                    ));
                    egui::ComboBox::from_id_salt("viewCopyIntoFile")
                        .selected_text(dialog.target.clone())
                        .show_ui(ui, |ui| {
                            for file in &files {
                                ui.selectable_value(&mut dialog.target, file.clone(), file);
                            }
                        });
                    if dialog.kind == CopyIntoKind::AsWrapper {
                        ui.horizontal(|ui| {
                            ui.label("Wrapper editor ID:");
                            ui.text_edit_singleline(&mut dialog.wrapper_editor_id);
                        });
                    }
                    ui.horizontal(|ui| {
                        copy = ui.button("Copy").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            if copy {
                self.run_copy_into(&dialog);
            } else if !cancel {
                self.copy_into = Some(dialog);
            }
        }
        if let Some(record) = self.confirm_delete.take() {
            let mut delete = false;
            let mut cancel = false;
            egui::Window::new("Remove record")
                .id(egui::Id::unique("viewConfirmDelete"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Are you sure you want to permanently remove {} from file \"{}\"?",
                        crate::view_grid::ViewModel::record_label(&record),
                        record.file
                    ));
                    ui.horizontal(|ui| {
                        delete = ui.button("Yes").clicked();
                        cancel = ui.button("No").clicked();
                    });
                });
            if delete {
                let removed = self.call_session_command(
                    "records.delete",
                    serde_json::json!({ "form_id": record.form_id, "file": record.file }),
                );
                if removed.is_some() {
                    self.reload_view();
                }
            } else if !cancel {
                self.confirm_delete = Some(record);
            }
        }
        if let Some(mut dialog) = self.create_mod_group.take() {
            let mut create = false;
            let mut cancel = false;
            egui::Window::new("Create ModGroup")
                .id(egui::Id::unique("viewCreateModGroup"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut dialog.name);
                    });
                    ui.horizontal(|ui| {
                        create = ui.button("Create").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            if create {
                self.run_create_mod_group(&dialog);
            } else if !cancel {
                self.create_mod_group = Some(dialog);
            }
        }
    }

    /// `CopyInto` of the record of a view column (`mniViewHeaderCopyIntoClick`).
    fn run_copy_into(&mut self, dialog: &CopyInto) {
        let (as_new, deep, overwrite, as_wrapper) = dialog.kind.request();
        let wrapper = (!dialog.wrapper_editor_id.is_empty()).then(|| dialog.wrapper_editor_id.clone());
        let copied = self.call_session_command(
            "records.copy",
            serde_json::json!({
                "form_id": dialog.record.form_id,
                "from": dialog.record.file,
                "to": dialog.target,
                "as_new": as_new,
                "deep": deep,
                "overwrite": overwrite,
                "as_wrapper": as_wrapper,
                "wrapper_editor_id": wrapper,
            }),
        );
        if copied.is_some() {
            self.reload_view();
        }
    }

    /// `mniViewCreateModGroupClick`: the override chain of the shown record
    /// (the master left out) as the modules of a new mod group.
    fn run_create_mod_group(&mut self, dialog: &CreateModGroup) {
        let modules: Vec<String> = self
            .view
            .model
            .records
            .iter()
            .skip(usize::from(self.view.model.kind == ViewKind::Single))
            .map(|record| record.file.clone())
            .collect();
        let created = self.call_session_command(
            "modgroups.create",
            serde_json::json!({
                "name": dialog.name,
                "modules": modules,
                "items": [],
                "include_crcs": false,
                "add_current_crcs": false,
            }),
        );
        if created.is_some() {
            self.mod_groups_present = true;
            self.add_message(&format!("ModGroup \"{}\" created", dialog.name));
        }
    }

    /// `GetAddElement`: the container and the name `elements.add` takes for
    /// the focused row. `None` when the row offers no member.
    fn view_add_target(&self) -> Option<(Option<String>, String)> {
        let column = self.focused_view_column()?;
        let mut row = self.view.state.focused_row?;
        let mut child: Option<usize> = None;
        loop {
            let view_row = self.view.model.rows[row];
            let node = &self.view.model.nodes[view_row.node];
            if let Some(cell) = node.cells.get(column)
                && let Some(path) = &cell.path
            {
                let child_node = child.unwrap_or(view_row.node);
                let child_view = &self.view.model.nodes[child_node];
                let name = match &child_view.member_name {
                    Some(name) => name.clone(),
                    None => child_view.index.to_string(),
                };
                return Some((Some(path.clone()), name));
            }
            child = Some(view_row.node);
            let parent = node.parent?;
            row = self.view.model.rows.iter().position(|row| row.node == parent)?;
        }
    }

    /// `mniViewAddClick`: adds the member the focused row is missing.
    fn view_add(&mut self) {
        let Some(record) = self.focused_view_record().cloned() else {
            return;
        };
        let Some((path, name)) = self.view_add_target() else {
            self.add_message("Add: the focused row has no member to add");
            return;
        };
        let added = self.call_session_command(
            "elements.add",
            serde_json::json!({
                "form_id": record.form_id,
                "file": record.file,
                "path": path,
                "name": name,
            }),
        );
        if added.is_some() {
            self.reload_view();
        }
    }

    /// `mniViewEditClick`: starts the in-place editor on the focused cell
    /// (`vstViewNewText` commits it through `elements.set`).
    fn view_edit(&mut self) {
        if !self.edit_allowed {
            return;
        }
        let Some(row) = self.view.state.focused_row else {
            return;
        };
        let column = self.view.state.focused_column;
        let Some(record_column) = self.focused_view_column() else {
            return;
        };
        let Some(cell) = self.view.model.cell(row, record_column) else {
            return;
        };
        if !cell.editable || cell.path.is_none() {
            self.add_message("The focused cell is not editable");
            return;
        }
        let grid_cell = crate::widgets::grid::GridCell {
            text: cell.value.clone().unwrap_or_default(),
            edit_text: cell.edit_value.clone(),
            editable: true,
            ..crate::widgets::grid::GridCell::default()
        };
        self.view.state.on_double_click(row, column, &grid_cell);
    }

    /// `mniViewSetToDefaultClick`: `elements.set` with a default value.
    fn view_set_to_default(&mut self) {
        let Some((cell, column)) = self.focused_view_cell() else {
            return;
        };
        let (Some(record), Some(path)) = (self.view.model.records.get(column).cloned(), cell.path.clone()) else {
            return;
        };
        let set = self.call_session_command(
            "elements.set",
            serde_json::json!({
                "form_id": record.form_id,
                "file": record.file,
                "path": path,
                "value": serde_json::Value::Null,
            }),
        );
        if set.is_some() {
            self.reload_view();
        }
    }

    /// `mniViewRemoveClick`, `mniViewClearClick`, the member switches and
    /// the moves: one command on the focused element's path.
    fn view_element_command(&mut self, command: &str, extra: serde_json::Value) {
        let Some((cell, column)) = self.focused_view_cell() else {
            return;
        };
        let (Some(record), Some(path)) = (self.view.model.records.get(column).cloned(), cell.path.clone()) else {
            return;
        };
        let mut params = serde_json::json!({
            "form_id": record.form_id,
            "file": record.file,
            "path": path,
        });
        if let (Some(params), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
            for (key, value) in extra {
                params.insert(key.clone(), value.clone());
            }
        }
        let ran = self.call_session_command(command, params);
        if ran.is_some() {
            self.reload_view();
        }
    }

    /// The other view columns that hold the same FormID: the targets of
    /// `mniViewRemoveFromSelected` and the copy items.
    fn view_columns_to_records(&self, column: usize) -> Vec<String> {
        let Some(source) = self.view.model.records.get(column) else {
            return Vec::new();
        };
        self.view
            .model
            .records
            .iter()
            .enumerate()
            .filter(|(index, record)| *index != column && record.form_id.eq_ignore_ascii_case(&source.form_id))
            .map(|(_, record)| record.file.clone())
            .collect()
    }

    fn view_remove_from_selected(&mut self) {
        let Some((cell, column)) = self.focused_view_cell() else {
            return;
        };
        let (Some(record), Some(path)) = (self.view.model.records.get(column).cloned(), cell.path.clone()) else {
            return;
        };
        let targets = self.view_columns_to_records(column);
        if targets.is_empty() {
            self.add_message("There is no other column of the same record to remove the element from");
            return;
        }
        let removed = self.call_session_command(
            "elements.remove_from_records",
            serde_json::json!({
                "form_id": record.form_id,
                "file": record.file,
                "path": path,
                "targets": targets,
            }),
        );
        if removed.is_some() {
            self.reload_view();
        }
    }

    fn view_copy_to_selected(&mut self, multiple: bool) {
        let Some((cell, column)) = self.focused_view_cell() else {
            return;
        };
        let (Some(record), Some(path)) = (self.view.model.records.get(column).cloned(), cell.path.clone()) else {
            return;
        };
        let targets = self.view_columns_to_records(column);
        if targets.is_empty() {
            self.add_message("There is no other column of the same record to copy the element into");
            return;
        }
        let mut params = serde_json::json!({
            "form_id": record.form_id,
            "file": record.file,
            "path": path,
            "targets": targets,
        });
        if multiple {
            params["mode"] = serde_json::json!("copy");
            params["deep"] = serde_json::json!(true);
            params["remove_missing"] = serde_json::json!(true);
        } else {
            params["mode"] = serde_json::json!("assign");
        }
        let copied = self.call_session_command("elements.copy_to_records", params);
        if copied.is_some() {
            self.reload_view();
        }
    }

    /// `mniViewSortClick`: a sibling comparison orders the columns by the
    /// focused row's sort keys and shows them again.
    fn view_sort(&mut self) {
        let Some(row) = self.view.state.focused_row else {
            return;
        };
        let Some(records) = self.view.model.sorted_records(row) else {
            return;
        };
        self.set_active_records(records);
    }

    /// `mniViewCompareReferencedRowClick`: shows the records the focused
    /// column's record refers to. UPSTREAM-QUIRK: the release compares only
    /// the references of the focused row's elements (`GetUniqueLinksTo`);
    /// `refs.get` reports the references of the whole record, so a row that
    /// is not the only one carrying references compares more records.
    fn view_compare_referenced_row(&mut self) {
        let Some(record) = self.focused_view_record().cloned() else {
            return;
        };
        let Some(result) = self.call_session_command(
            "refs.get",
            serde_json::json!({ "form_id": record.form_id, "file": record.file }),
        ) else {
            return;
        };
        let mut records: Vec<ViewRecord> = Vec::new();
        if let Some(references) = result.get("references").and_then(|value| value.as_array()) {
            for reference in references {
                let Some(target) = reference.get("record") else {
                    continue;
                };
                let (Some(form_id), Some(file)) = (
                    target.get("form_id").and_then(|value| value.as_str()),
                    target.get("file").and_then(|value| value.as_str()),
                ) else {
                    continue;
                };
                let record = ViewRecord {
                    form_id: form_id.to_owned(),
                    file: file.to_owned(),
                };
                if !records.contains(&record) {
                    records.push(record);
                }
            }
        }
        if records.len() < 2 {
            self.add_message("The focused row refers to fewer than two records");
            return;
        }
        self.set_active_records(records);
    }

    /// The clipboard items of `pmuView`: the focused element's texts.
    fn copy_view_text(&mut self, ctx: &egui::Context, id: &'static str) {
        let Some((cell, column)) = self.focused_view_cell() else {
            return;
        };
        let Some(record) = self.view.model.records.get(column).cloned() else {
            return;
        };
        let path = cell.path.clone().unwrap_or_default();
        let text = match id {
            // `Element.Path`: the names from the file down. The view model
            // keeps the path inside the record.
            "mniCopyPathToClipboard" => path.replace('\\', " \\ "),
            "mniCopyNameToClipboard" | "mniCopyShortNameToClipboard" => cell.name.clone().unwrap_or_default(),
            "mniCopyDisplayNameToClipboard" => cell.display_name.clone().unwrap_or_default(),
            "mniCopyFullPathToClipboard"
            | "mniCopySignatureToClipboard"
            | "mniCopyIndexedPathToClipBoard"
            | "mniCopyPathNameToClipboard" => {
                let result = self.call_session_command(
                    "elements.get",
                    serde_json::json!({
                        "form_id": record.form_id,
                        "file": record.file,
                        "path": path,
                        "depth": 0,
                    }),
                );
                match (id, result) {
                    ("mniCopyFullPathToClipboard", Some(result)) => result
                        .get("path")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    ("mniCopySignatureToClipboard", Some(result)) => result
                        .get("signature")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    _ => {
                        self.add_message("This clipboard text needs an element path the port does not have");
                        return;
                    }
                }
            }
            _ => return,
        };
        ctx.copy_text(text.clone());
        self.add_message(&format!("Copied to the clipboard: {text}"));
    }

    /// `stbMain`: the last message and the progress gauge of a load.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let _status = egui::Panel::bottom("stbMain").show(ui, |ui| {
            ui.horizontal(|ui| {
                if matches!(self.startup, Startup::Loading) {
                    ui.add(
                        egui::ProgressBar::new(0.0)
                            .desired_width(180.0)
                            .animate(true)
                            .text(format!("{:.0} s", self.load_started.elapsed().as_secs_f32())),
                    );
                }
                ui.label(self.status_text());
            });
        });
        #[cfg(test)]
        self.rects.push(("stbMain", _status.response.rect));
    }

    /// Opens the small target-file dialog of a copy item (`xeFileSelectForm`
    /// of step 5 stands in for it).
    fn open_copy_into(&mut self, kind: CopyIntoKind) {
        let Some(record) = self.header_view_record().cloned() else {
            return;
        };
        let target = self
            .files
            .iter()
            .find(|file| !file.eq_ignore_ascii_case(&record.file))
            .cloned()
            .unwrap_or_default();
        self.copy_into = Some(CopyInto {
            record,
            kind,
            target,
            wrapper_editor_id: String::new(),
        });
    }

    /// Runs the view items of `pmuView`, `pmuViewHeader` and the clipboard
    /// items. Returns whether the item belongs to the view.
    fn run_view_command(&mut self, ctx: &egui::Context, id: &'static str) -> bool {
        match id {
            "mniViewEdit" => self.view_edit(),
            "mniViewSetToDefault" => self.view_set_to_default(),
            "mniViewAdd" => self.view_add(),
            "mniViewRemove" => self.view_element_command("elements.remove", serde_json::json!({})),
            "mniViewClear" => self.view_element_command("elements.clear", serde_json::json!({})),
            "mniViewNextMember" => {
                self.view_element_command("elements.change_member", serde_json::json!({ "direction": "next" }))
            }
            "mniViewPreviousMember" => {
                self.view_element_command("elements.change_member", serde_json::json!({ "direction": "previous" }))
            }
            "mniViewMoveUp" => self.view_element_command("elements.move", serde_json::json!({ "direction": "up" })),
            "mniViewMoveDown" => self.view_element_command("elements.move", serde_json::json!({ "direction": "down" })),
            "mniViewRemoveFromSelected" => self.view_remove_from_selected(),
            "mniViewCopyToSelectedRecords" => self.view_copy_to_selected(false),
            "mniViewCopyMultipleToSelectedRecords" => self.view_copy_to_selected(true),
            "mniViewSort" => self.view_sort(),
            "mniViewCompareReferencedRow" => self.view_compare_referenced_row(),
            "mniViewHideNoConflict" => {
                self.settings.view.hide_no_conflict = !self.settings.view.hide_no_conflict;
                self.add_message(&format!(
                    "Hide no conflict and empty rows: {}",
                    if self.settings.view.hide_no_conflict {
                        "on"
                    } else {
                        "off"
                    }
                ));
                self.reload_view();
            }
            "mniMasterAndLeafsEnabled" => {
                self.view_master_and_leafs = true;
                self.reload_view();
            }
            "mniMasterAndLeafsDisabled" => {
                self.view_master_and_leafs = false;
                self.reload_view();
            }
            "mniModGroupsEnabled" => {
                self.view_mod_groups = true;
                self.reload_view();
            }
            "mniModGroupsDisabled" => {
                self.view_mod_groups = false;
                self.reload_view();
            }
            "mniViewModGroupsReload" => {
                let reloaded = self.call_session_command("modgroups.select", serde_json::json!({ "saved": true }));
                if reloaded.is_some() {
                    self.add_message("Mod groups reloaded");
                    self.reload_view();
                }
            }
            "mniViewHeaderCopyAsOverride" => self.open_copy_into(CopyIntoKind::Override),
            "mniViewHeaderCopyAsOverrideWithOverwriting" => self.open_copy_into(CopyIntoKind::OverrideOverwrite),
            "mniViewHeaderDeepCopyAsOverride" => self.open_copy_into(CopyIntoKind::DeepOverride),
            "mniViewHeaderDeepCopyAsOverrideWithOverwriting" => {
                self.open_copy_into(CopyIntoKind::DeepOverrideOverwrite)
            }
            "mniViewHeaderCopyAsNewRecord" => self.open_copy_into(CopyIntoKind::AsNew),
            "mniViewHeaderCopyAsWrapper" => self.open_copy_into(CopyIntoKind::AsWrapper),
            "mniViewHeaderRemove" => {
                if let Some(record) = self.header_view_record().cloned() {
                    self.confirm_delete = Some(record);
                }
            }
            "mniViewHeaderJumpTo" => {
                // `mniViewHeaderJumpToClick`: `JumpTo(MainRecord, False)`
                // shows the record in the navigation tree.
                self.jump_to_navigation_tree();
            }
            "mniViewCreateModGroup" => {
                self.create_mod_group = Some(CreateModGroup { name: String::new() });
            }
            "mniViewHeaderHidden" => {
                if let Some(record) = self.header_view_record().cloned() {
                    let hidden = self.call_session_command(
                        "records.set_hidden",
                        serde_json::json!({
                            "form_id": record.form_id,
                            "file": record.file,
                            "hidden": true,
                        }),
                    );
                    if hidden.is_some() {
                        self.reload_view();
                    }
                }
            }
            "mniViewHeaderUnhideAll" => {
                if let Some(record) = self.header_view_record().cloned() {
                    let shown = self.call_session_command(
                        "records.set_hidden",
                        serde_json::json!({
                            "form_id": record.form_id,
                            "file": record.file,
                            "hidden": false,
                            "overrides": true,
                        }),
                    );
                    if shown.is_some() {
                        self.reload_view();
                    }
                }
            }
            "mniCopyPathToClipboard"
            | "mniCopyFullPathToClipboard"
            | "mniCopyIndexedPathToClipBoard"
            | "mniCopyPathNameToClipboard"
            | "mniCopySignatureToClipboard"
            | "mniCopyNameToClipboard"
            | "mniCopyDisplayNameToClipboard"
            | "mniCopyShortNameToClipboard" => self.copy_view_text(ctx, id),
            _ => return false,
        }
        true
    }

    /// Runs a menu command the form handles itself.
    fn run_menu_command(&mut self, ctx: &egui::Context, id: &'static str, command: MenuCommand) {
        // `pnlClient.Enabled := False` while an operation runs: the menus
        // of the form do not start a second one.
        if self.job.is_running() {
            self.add_message(&format!(
                "{} is still running: wait for it, or press Cancel in its wait form",
                self.job.running_command().unwrap_or("a command")
            ));
            return;
        }
        if self.run_view_command(ctx, id) {
            return;
        }
        match command {
            MenuCommand::SessionWithParam { command, param } => match command {
                // `mniMainLocalizationLanguageClick`: the language of the
                // string tables, chosen from the run-time list.
                "localization.language" => self.switch_language(&param),
                _ => self.run_session_command(command, json!({}), None),
            },
            MenuCommand::Session(name) => match name {
                // `mniNavFilterApplyClick`: the options dialog, then
                // `filter.apply` on its answer.
                "filter.apply" => self.open_filter_options(),
                // `mniViewModGroupsReloadClick`: the selection dialog, then
                // `modgroups.select`.
                "modgroups.select" => self.open_mod_group_select(GroupSelectFlow::Reload),
                // `mniNavEditModGroupClick`: the selection dialog, then the
                // edit dialog and `modgroups.edit`.
                "modgroups.edit" => self.open_mod_group_select(GroupSelectFlow::Edit),
                // `mniNavGenerateLODClick`: the LODGen form.
                "lodgen.generate" => self.open_lodgen(),
                // `mniNavBuildRefClick`: the references off the UI thread.
                "refs.build" => {
                    let spec = JobSpec {
                        caption: "Building Reference Info".to_owned(),
                        message: "Building reference information. Please Wait...".to_owned(),
                        can_cancel: true,
                        show_delay: 200,
                        update_delay: 100,
                    };
                    self.start_job("refs.build", serde_json::json!({}), spec, PendingJob::Command);
                }
                // Step 6's viewers and editors.
                "script.run" => self.open_dialog(DialogKind::Script),
                "logs.analyze" => self.open_dialog(DialogKind::LogAnalyzer),
                "localization.files" => self.open_dialog(DialogKind::Localization),
                "localization.localize" => self.open_dialog(DialogKind::LocalizePlugin),
                // Every other action is a session command (design rule 1);
                // the request of the `pmuMain` and button-strip items
                // carries no parameters.
                _ => self.run_session_command(name, json!({}), None),
            },
            MenuCommand::Presentation(id) => match id {
                "mniViewColumnWidthStandard" => {
                    self.settings.view.column_width = crate::settings::ColumnWidthMode::Standard;
                    self.update_column_widths();
                }
                "mniViewColumnWidthFitAll" => {
                    self.settings.view.column_width = crate::settings::ColumnWidthMode::FitAll;
                    self.update_column_widths();
                }
                "mniViewColumnWidthFitText" => {
                    self.settings.view.column_width = crate::settings::ColumnWidthMode::FitText;
                    self.update_column_widths();
                }
                "mniViewColumnWidthFitSmart" => {
                    self.settings.view.column_width = crate::settings::ColumnWidthMode::FitSmart;
                    self.update_column_widths();
                }
                "mniViewStickAuto" => self.stick_follows_selection = false,
                "mniViewStickSelected" => self.stick_follows_selection = true,
                "mniNavHeaderFilesDefault" => {
                    self.settings.view.files_sort = crate::settings::FilesSort::AsSelected;
                    self.nav.sort_roots(self.settings.view.files_sort);
                }
                "mniNavHeaderFilesLoadOrder" => {
                    self.settings.view.files_sort = crate::settings::FilesSort::LoadOrder;
                    self.nav.sort_roots(self.settings.view.files_sort);
                }
                "mniNavHeaderFilesFileName" => {
                    self.settings.view.files_sort = crate::settings::FilesSort::FileName;
                    self.nav.sort_roots(self.settings.view.files_sort);
                }
                "mniNavHeaderINFObyPreviousINFO" => {
                    self.settings.view.info_sort = crate::settings::InfoSort::ByPreviousInfo
                }
                "mniNavHeaderINFObyFormID" => {
                    self.settings.view.info_sort = crate::settings::InfoSort::ByFormId
                }
                // `mniBtnShrinkButtonsClick`: `wbShrinkButtons` is written to
                // the settings file under `Options`/`ShrinkButtons`, so the
                // next run starts with the buttons shrunk.
                "mniBtnShrinkButtons" => {
                    self.shrink_buttons = !self.shrink_buttons;
                    self.write_settings(
                        "Options",
                        "ShrinkButtons",
                        serde_json::Value::Bool(self.shrink_buttons),
                        "bool",
                    );
                }
                // `mniNavOptionsClick` and `mniMainOptions`.
                "mniMainOptions" | "mniNavOptions" => self.open_options(),
                // `mniNavUndeleteAndDisableReferencesClick`: `files.clean`
                // with `udr`, off the UI thread.
                "mniNavUndeleteAndDisableReferences" => {
                    let spec = JobSpec {
                        caption: "Cleaning".to_owned(),
                        message: "Undeleting and Disabling References. Please wait...".to_owned(),
                        can_cancel: true,
                        show_delay: 200,
                        update_delay: 100,
                    };
                    self.start_job(
                        "files.clean",
                        serde_json::json!({ "udr": true }),
                        spec,
                        PendingJob::Command,
                    );
                }
                // `mniNavRemoveIdenticalToMasterClick`: `files.clean` with
                // `itm`.
                "mniNavRemoveIdenticalToMaster" => {
                    let spec = JobSpec {
                        caption: "Cleaning".to_owned(),
                        message: "Removing Identical to Master records. Please wait...".to_owned(),
                        can_cancel: true,
                        show_delay: 200,
                        update_delay: 100,
                    };
                    self.start_job(
                        "files.clean",
                        serde_json::json!({ "itm": true }),
                        spec,
                        PendingJob::Command,
                    );
                }
                "cleaning_obsolete" => self.add_message(
                    "Cleaning moved to the new commands: use Apply Filter for Cleaning, Undelete and Disable References, Remove \"Identical to Master\" records.",
                ),
                _ => {}
            },
            MenuCommand::Excluded => {
                self.add_message("This integration is not offered by the port (see the coverage ledger).")
            }
        }
    }

    /// Runs one session command and reports its result to the message log.
    fn call_session(&mut self, name: &str, params: Value) -> Result<Value, CommandError> {
        let Some(bridge) = self.bridge.as_mut() else {
            return Err(CommandError::new("no_session", "no session is loaded"));
        };
        bridge.call(name, params)
    }

    /// [`TfrmMain::call_session`] with the messages every action writes.
    fn run_session_command(&mut self, command: &'static str, params: Value, message: Option<String>) {
        match self.call_session(command, params) {
            Ok(value) => {
                if let Some(message) = message {
                    self.add_message(&message);
                }
                self.after_command(command, &value);
            }
            Err(error) => {
                let error = format!("{command}: {}", error.message);
                self.add_message(&error);
            }
        }
    }

    /// What the form does with a finished command: the filter view, the
    /// message lines, and the `NavUpdate` after a command that changed the
    /// tree.
    fn after_command(&mut self, command: &str, value: &Value) {
        match command {
            "filter.apply" => {
                match serde_json::from_value::<xedit_session::filter::FilterApplyResponse>(value.clone()) {
                    Ok(response) => {
                        for message in &response.messages {
                            self.add_message(message);
                        }
                        self.add_message(&format!(
                            "[Pass 1] Processed Records: {}, [Pass 2] Processed Records: {}, Remaining unfiltered nodes: {}",
                            response.pass1, response.pass2, response.unfiltered
                        ));
                        let statuses = response
                            .node_conflicts
                            .iter()
                            .map(|node| {
                                (
                                    node.element,
                                    ConflictAll::from(node.conflict_all),
                                    ConflictThis::from(node.conflict_this),
                                )
                            })
                            .collect();
                        self.nav.install_filter(response.nodes.clone(), statuses);
                        // `ReInitTree` cleared the tree, so the selection
                        // does not survive the filter.
                        self.nav_state.selected.clear();
                        self.nav_state.focused = None;
                        self.nav_state.anchor = None;
                    }
                    Err(error) => self.add_message(&format!("filter.apply: {error}")),
                }
                return;
            }
            "filter.remove" => {
                self.nav.remove_filter();
                self.nav_state.selected.clear();
                self.nav_state.focused = None;
                self.nav_state.anchor = None;
                self.add_message("Filter removed");
                return;
            }
            "leveled_lists.check_circular" => {
                if let Some(cycles) = value["cycles"].as_array() {
                    for cycle in cycles {
                        if let Some(cycle) = cycle.as_str() {
                            self.add_message(cycle);
                        }
                    }
                }
                self.add_message(&format!(
                    "Checked {} leveled lists",
                    value["checked"].as_u64().unwrap_or_default()
                ));
            }
            "files.save" => {
                let saved = value["saved"].as_array().cloned().unwrap_or_default();
                if saved.is_empty() {
                    self.add_message("No plugin needed saving");
                } else {
                    for file in saved {
                        if let Some(file) = file.as_str() {
                            self.add_message(&format!("Saving: {file}"));
                        }
                    }
                }
            }
            _ => {}
        }
        // The commands of the tree write the lines xEdit prints (`messages`
        // of `files.clean`, `files.check`, `files.dirty_info`, `refs.build`
        // and the rest).
        if !matches!(command, "filter.apply" | "filter.remove") {
            if let Some(messages) = value["messages"].as_array() {
                for message in messages {
                    if let Some(text) = message.as_str() {
                        self.add_message(text);
                    }
                }
            }
            // `refs.build` reports its lines per file (`FileRefs.message`).
            if let Some(files) = value["files"].as_array() {
                for file in files {
                    if let Some(text) = file["message"].as_str() {
                        self.add_message(text);
                    }
                }
            }
        }
        // `NavUpdate(True)` after an edit: a collapsed or expanded subtree
        // that changed is rebuilt when it is next shown.
        let mutates = self
            .bridge
            .as_ref()
            .and_then(|bridge| bridge.mutates(command))
            .unwrap_or(false);
        // `mniNavHiddenClick` also resets the conflict colours of the
        // nodes (`NodeData.ConflictAll := caUnknown`).
        if (mutates || command == "records.set_hidden") && command != "files.save" {
            self.nav.refresh();
        }
    }

    /// Runs the actions a navigation menu item resolved into.
    fn run_nav_actions(&mut self, ctx: &egui::Context, actions: Vec<NavAction>) {
        for action in actions {
            match action {
                NavAction::Command {
                    command,
                    params,
                    message,
                } => {
                    // The cleaning pass of `files.clean` runs off the UI
                    // thread like the menu paths of step 7 (`TfrmWait` with
                    // its Cancel).
                    if command == "files.clean" {
                        if let Some(message) = message {
                            self.add_message(&message);
                        }
                        let spec = JobSpec {
                            caption: "Cleaning".to_owned(),
                            message: "Cleaning the selected files. Please wait...".to_owned(),
                            can_cancel: true,
                            show_delay: 200,
                            update_delay: 100,
                        };
                        self.start_job(command, params, spec, PendingJob::Command);
                    } else {
                        self.run_session_command(command, params, message);
                    }
                }
                NavAction::Confirm { message, then } => {
                    self.confirm = Some(ConfirmState { message, actions: then });
                }
                NavAction::Flow(flow) => self.run_nav_flow(flow),
                NavAction::Dialog(dialog) => self.open_nav_dialog(dialog),
                NavAction::Presentation(id) => self.run_menu_command(ctx, id, MenuCommand::Presentation(id)),
                NavAction::Message(text) => {
                    for line in text.split("\r\n") {
                        self.add_message(line);
                    }
                }
                NavAction::None => {}
            }
        }
    }

    // ----- Step 8: the nav flows (F9) -----

    /// `mniNavCompareToClick` and the rest of the navigation items that
    /// open a dialog before their command: the flow builds its asks and
    /// opens the first one.
    fn run_nav_flow(&mut self, flow: NavFlow) {
        let asks = self.nav_flow_asks(&flow);
        self.nav_flow = Some(NavFlowState {
            flow,
            asks,
            answers: Vec::new(),
            index: 0,
        });
        self.advance_nav_flow();
    }

    /// Opens the ask the flow is at, or runs the command when every ask
    /// is answered.
    fn advance_nav_flow(&mut self) {
        let Some(state) = self.nav_flow.as_ref() else {
            return;
        };
        let flow = state.flow.clone();
        let answers = state.answers.clone();
        let index = state.index;
        let Some(mut ask) = state.asks.get(index).cloned() else {
            self.finish_nav_flow();
            return;
        };
        if let NavFlow::DeltaPatch { .. } = &flow {
            // `mniNavCreateDeltaPatchClick`: the delta patch is only made
            // for a module.
            if index == 1 {
                let chosen = match answers.first() {
                    Some(NavAnswer::Path(path)) => path.clone(),
                    _ => String::new(),
                };
                if !self.is_module_file(&chosen) {
                    self.add_message("Delta patch can only be created for modules");
                    self.nav_flow = None;
                    return;
                }
            }
        }
        if let NavFlow::UpdateCrc = &flow {
            match index {
                // The second question is only asked when the first one was
                // answered yes. When it was not, the items that are
                // missing the current CRC are still updated.
                1 => {
                    if matches!(answers.first(), Some(NavAnswer::Yes(false))) {
                        self.nav_flow_answer(NavAnswer::Yes(true));
                        return;
                    }
                }
                // The caption of the module list follows the answers.
                2 => {
                    if let NavAsk::Files { caption, .. } = &mut ask {
                        *caption = update_crc_caption(&answers);
                        let caption = caption.clone();
                        if let Some(state) = self.nav_flow.as_mut()
                            && let Some(NavAsk::Files { caption: stored, .. }) = state.asks.get_mut(index)
                        {
                            *stored = caption;
                        }
                    }
                }
                _ => {}
            }
        }
        if let NavFlow::CopyInto { kind, records } = &flow {
            // The number of EditorID boxes the flow asks: one for a single
            // record, four for more than one (`CopyInto`,
            // xeMainForm.pas:3462-3506).
            let boxes = if records.len() == 1 { 1 } else { 4 };
            if index == boxes {
                let original_editor_id = records
                    .first()
                    .map(|(form_id, _)| self.record_editor_id(form_id))
                    .unwrap_or_default();
                if let Some(ask) = copy_into_repeat(*kind, records.len() == 1, &original_editor_id, &answers) {
                    match ask {
                        CopyIntoRepeat::Question(message) => {
                            // The warning of an unchanged EditorID: the
                            // copy continues when the user confirms it and
                            // the boxes are asked again when they do not.
                            self.nav_question = Some(NavQuestionState {
                                message,
                                default_yes: false,
                            });
                            self.nav_question_is_copy = true;
                            return;
                        }
                        CopyIntoRepeat::Message(text) => {
                            // `ShowMessage` of the copy flow: the message
                            // is shown and the boxes are asked again.
                            self.add_message(&text);
                            self.copy_repeat_asks();
                            return;
                        }
                    }
                }
            }
        }
        match ask {
            NavAsk::Text {
                caption,
                message,
                value,
                require_text,
            } => {
                let mut prompt = TextPromptState::new(caption, message, value);
                prompt.require_text = require_text;
                self.dialog = Some(Dialog::TextPrompt(Box::new(prompt)));
            }
            NavAsk::Path {
                caption,
                message,
                initial_dir,
                extension,
            } => {
                // `odModule` fills the list with the files of the folder
                // that carry the extension.
                let candidates = self.path_candidates(&initial_dir, &extension);
                let mut prompt = PathPromptState::new(caption, message, initial_dir, extension);
                prompt.set_candidates(candidates);
                self.dialog = Some(Dialog::PathPrompt(Box::new(prompt)));
            }
            NavAsk::Files {
                caption,
                items,
                checked,
                single,
            } => {
                let mut state = FileSelectState::new(caption, items);
                for item in &mut state.items {
                    item.checked = checked.contains(&item.text);
                }
                // `TfrmModuleSelect`'s `MaxSelect` of one: the list of a
                // single target file holds one name only. The list of the
                // port is a check list, so the answer is trimmed to the
                // first name when the ask wants one.
                state.backup_visible = false;
                self.nav_flow_single = single;
                self.dialog = Some(Dialog::FileSelect(Box::new(state)));
            }
            NavAsk::ModGroups { caption, single } => {
                let groups = match self.dialog_command(|bridge| {
                    bridge.request::<_, ModGroupListResponse>("modgroups.list", &ModGroupListRequest { all: true })
                }) {
                    Ok(list) => list.mod_groups,
                    Err(error) => {
                        self.add_message(&format!("Fatal: {}", error.message));
                        self.nav_flow = None;
                        return;
                    }
                };
                let mut state = ModGroupSelectState::new(
                    groups,
                    Vec::new(),
                    false,
                    &[],
                    caption,
                    false,
                    usize::from(single),
                    usize::from(single).max(1),
                );
                state.presets_visible = false;
                self.dialog = Some(Dialog::ModGroupSelect(Box::new(state)));
            }
            NavAsk::Question { message, default_yes } => {
                self.nav_question = Some(NavQuestionState { message, default_yes });
            }
        }
    }

    /// The files of a folder that carry an extension, for the open dialog
    /// of a flow.
    fn path_candidates(&self, folder: &str, extension: &str) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
            .filter(|name| {
                let name = name.to_ascii_lowercase();
                extension.is_empty() || name.ends_with(&extension.to_ascii_lowercase())
            })
            .collect();
        names.sort();
        names
    }

    /// The asks of a flow, in the order upstream asks them.
    fn nav_flow_asks(&mut self, flow: &NavFlow) -> Vec<NavAsk> {
        match flow {
            NavFlow::CompareTo { file } => vec![NavAsk::Path {
                caption: format!("Please select the file you want to compare to \"{file}\"..."),
                message: String::new(),
                initial_dir: self.data_path.clone(),
                extension: "esp".to_owned(),
            }],
            NavFlow::DeltaPatch { file } => vec![
                NavAsk::Path {
                    caption: format!("Please select a newer version of \"{file}\" to create a delta patch..."),
                    message: String::new(),
                    initial_dir: self.data_path.clone(),
                    extension: "esp".to_owned(),
                },
                NavAsk::Text {
                    caption: "Delta Patch Filename".to_owned(),
                    message: "Please specify the name of the delta patch (without extension)".to_owned(),
                    value: String::new(),
                    require_text: true,
                },
            ],
            NavFlow::BatchChange { .. } => vec![NavAsk::Path {
                caption: "The CSV file of the batch reference change...".to_owned(),
                message: String::new(),
                initial_dir: self.data_path.clone(),
                extension: "csv".to_owned(),
            }],
            NavFlow::AddMasters { file } => {
                let modules = self.nav_modules();
                vec![NavAsk::Files {
                    caption: "Which masters do you want to add?".to_owned(),
                    items: add_master_candidates(&modules, file),
                    checked: Vec::new(),
                    single: false,
                }]
            }
            NavFlow::BuildRef => {
                let modules = self.nav_modules();
                vec![NavAsk::Files {
                    caption: "Build reference information for:".to_owned(),
                    items: build_ref_candidates(&modules),
                    checked: Vec::new(),
                    single: false,
                }]
            }
            NavFlow::ChangeFormId { form_id, .. } => {
                let name = self.record_name(form_id);
                vec![
                    NavAsk::Text {
                        caption: "New FormID".to_owned(),
                        message: "Please enter the new FormID in hex. e.g. 0404CC43. The FormID needs to be a load order corrected form ID.".to_owned(),
                        value: form_id.clone(),
                        require_text: true,
                    },
                    NavAsk::Question {
                        message: format!("Record {name} has later overrides, update them too?"),
                        default_yes: true,
                    },
                ]
            }
            NavFlow::ChangeFormIdInto { form_id, file } => {
                let modules = self.nav_modules();
                let name = self.record_name(form_id);
                vec![
                    NavAsk::Files {
                        caption: "Select a single target file".to_owned(),
                        items: change_form_id_candidates(&modules, file),
                        checked: Vec::new(),
                        single: true,
                    },
                    NavAsk::Text {
                        caption: "New FormID".to_owned(),
                        message: "Please enter the new FormID in hex. e.g. 0404CC43. The FormID needs to be a load order corrected form ID.".to_owned(),
                        value: form_id.clone(),
                        require_text: true,
                    },
                    NavAsk::Question {
                        message: format!("Record {name} has later overrides, update them too?"),
                        default_yes: true,
                    },
                ]
            }
            NavFlow::ChangeReferencingRecords { form_id, .. } => vec![NavAsk::Text {
                caption: "New FormID".to_owned(),
                message: "Please enter the new FormID in hex. e.g. 0404CC43. The FormID needs to be a load order corrected form ID.".to_owned(),
                value: form_id.clone(),
                require_text: true,
            }],
            NavFlow::RenumberFrom { .. } => vec![NavAsk::Text {
                caption: "Start from...".to_owned(),
                message: "Please enter the new module specific start FormID in hex. e.g. 200000. Specify only the last 6 digits.".to_owned(),
                value: String::new(),
                require_text: true,
            }],
            NavFlow::RenumberInject { file } => {
                let modules = self.nav_modules();
                let masters = change_form_id_candidates(&modules, file);
                vec![
                    NavAsk::Files {
                        caption: "Please select the master you want to inject new records into...".to_owned(),
                        items: masters,
                        checked: Vec::new(),
                        single: true,
                    },
                    NavAsk::Text {
                        caption: "Start from...".to_owned(),
                        message: "Please enter the new module specific start FormID in hex. e.g. 200000. Specify only the last 6 digits.".to_owned(),
                        value: String::new(),
                        require_text: true,
                    },
                ]
            }
            NavFlow::SetVwdInto { records } => {
                let modules = self.nav_modules();
                let source = records.first().map(|(_, file)| file.clone()).unwrap_or_default();
                vec![NavAsk::Files {
                    caption: "Set VWD for all REFR with VWD Mesh as Override into....".to_owned(),
                    items: copy_target_candidates(&modules, &source),
                    checked: Vec::new(),
                    single: false,
                }]
            }
            NavFlow::UpdateCrc => {
                let modules = self.nav_modules();
                vec![
                    NavAsk::Question {
                        message: "Do you wish to add CRCs to ModGroup Items which currently do not contain any CRCs?".to_owned(),
                        default_yes: true,
                    },
                    NavAsk::Question {
                        message: "Do you wish to update CRCs in ModGroup Items which already contain CRCs, but are missing the current one?".to_owned(),
                        default_yes: true,
                    },
                    NavAsk::Files {
                        caption: "Please select the modules for which you want to add or update the current CRC to/in ModGroup Items...".to_owned(),
                        items: update_crc_candidates(&modules),
                        checked: Vec::new(),
                        single: false,
                    },
                    NavAsk::ModGroups {
                        caption: "Please select the ModGroups you wish to update...".to_owned(),
                        single: false,
                    },
                ]
            }
            NavFlow::CreateModGroup { name, modules } => {
                let storage = self.nav_modules();
                vec![
                    NavAsk::Text {
                        caption: "ModGroup Name".to_owned(),
                        message: "Please enter the name of the new ModGroup".to_owned(),
                        value: name.clone(),
                        require_text: true,
                    },
                    NavAsk::Files {
                        caption: "Which modules should be in the ModGroup?".to_owned(),
                        items: modules.clone(),
                        checked: Vec::new(),
                        single: false,
                    },
                    NavAsk::Files {
                        caption: "In which module's .modgroups file should the new ModGroup be stored?".to_owned(),
                        items: update_crc_candidates(&storage),
                        checked: Vec::new(),
                        single: true,
                    },
                ]
            }
            NavFlow::DeleteModGroups => vec![NavAsk::ModGroups {
                caption: "Which ModGroups do you want to delete?".to_owned(),
                single: false,
            }],
            NavFlow::CopyIdle { file } => {
                let prefixes = self.idle_prefixes(file);
                // The "Model Prefix" box follows the prefixes the user
                // chose, so the rest of the asks are built when the first
                // answer is in (`nav_copy_idle_asks`).
                vec![NavAsk::Files {
                    caption: "Which Idles do you want to copy?".to_owned(),
                    items: prefixes,
                    checked: Vec::new(),
                    single: false,
                }]
            }
            NavFlow::MergedPatch { .. } => vec![NavAsk::Text {
                caption: "New Module File".to_owned(),
                message: "Filename without extension:".to_owned(),
                value: String::new(),
                require_text: true,
            }],
            NavFlow::NewFile => vec![
                NavAsk::Files {
                    caption: "What type of module do you want to create?".to_owned(),
                    items: file_templates(),
                    checked: Vec::new(),
                    single: true,
                },
                NavAsk::Text {
                    caption: "New Module File".to_owned(),
                    message: "Filename without extension:".to_owned(),
                    value: String::new(),
                    require_text: true,
                },
            ],
            NavFlow::CopyInto { kind, records } => {
                let modules = self.nav_modules();
                let source = records.first().map(|(_, file)| file.clone()).unwrap_or_default();
                let mut asks = Vec::new();
                let single = records.len() == 1;
                let editor_id = records
                    .first()
                    .map(|(form_id, _)| self.record_editor_id(form_id))
                    .unwrap_or_default();
                match kind {
                    NavCopyIntoKind::AsNew if single => {
                        // `CopyInto` for one record: the EditorID box
                        // repeats until the answer differs from the
                        // original, and the warning is shown when the
                        // user keeps it (see `advance_nav_flow`).
                        asks.push(NavAsk::Text {
                            caption: "EditorID".to_owned(),
                            message: "Please change the EditorID".to_owned(),
                            value: editor_id.clone(),
                            require_text: true,
                        });
                    }
                    NavCopyIntoKind::AsWrapper if single => {
                        asks.push(NavAsk::Text {
                            caption: "EditorID".to_owned(),
                            message: "Please enter the EditorID for the wrapped copy".to_owned(),
                            value: editor_id.clone(),
                            require_text: true,
                        });
                    }
                    NavCopyIntoKind::AsNew | NavCopyIntoKind::AsWrapper => {
                        // The four prompts of upstream's order, for more
                        // than one record.
                        asks.push(NavAsk::Text {
                            caption: "EditorID Prefix".to_owned(),
                            message: "Please enter the prefix that should be removed from the EditorIDs if present".to_owned(),
                            value: String::new(),
                            require_text: false,
                        });
                        asks.push(NavAsk::Text {
                            caption: "EditorID Suffix".to_owned(),
                            message: "Please enter the suffix that should be removed from the EditorIDs if present".to_owned(),
                            value: String::new(),
                            require_text: false,
                        });
                        asks.push(NavAsk::Text {
                            caption: "EditorID Prefix".to_owned(),
                            message: "Please enter the prefix that should be added to EditorIDs".to_owned(),
                            value: String::new(),
                            require_text: false,
                        });
                        asks.push(NavAsk::Text {
                            caption: "EditorID Suffix".to_owned(),
                            message: "Please enter the suffix that should be added to EditorIDs".to_owned(),
                            value: String::new(),
                            require_text: false,
                        });
                    }
                    _ => {}
                }
                asks.push(NavAsk::Files {
                    caption: kind.caption(single),
                    items: copy_target_candidates(&modules, &source),
                    checked: Vec::new(),
                    single: false,
                });
                asks
            }
            NavFlow::FilterApply { files, conflicts } => {
                let mut asks = Vec::new();
                if files.is_empty() {
                    asks.push(NavAsk::Files {
                        caption: "What files should be added to the navigation treeview before applying the filter?".to_owned(),
                        items: self.files.clone(),
                        checked: self.files.clone(),
                        single: false,
                    });
                }
                let _ = conflicts;
                asks
            }
        }
    }

    /// The modules of the data folder, for the module pickers of a flow.
    fn nav_modules(&mut self) -> Vec<xedit_session::module_select::ModuleEntry> {
        match self.dialog_command(|bridge| {
            bridge.request::<_, ModulesListResponse>(
                "modules.list",
                &ModulesListRequest {
                    valid: true,
                    active: false,
                    masters_present: false,
                },
            )
        }) {
            Ok(list) => list.modules,
            Err(error) => {
                self.add_message(&format!("Fatal: {}", error.message));
                Vec::new()
            }
        }
    }

    /// The name of a record, for the question about its later overrides.
    fn record_name(&mut self, form_id: &str) -> String {
        match self.call_session("records.get", json!({ "form_id": form_id, "depth": 0 })) {
            Ok(value) => value["name"].as_str().unwrap_or(form_id).to_owned(),
            Err(_) => form_id.to_owned(),
        }
    }

    /// The editor ID of a record, for the `EditorID` prompt of a copy.
    fn record_editor_id(&mut self, form_id: &str) -> String {
        match self.call_session("records.get", json!({ "form_id": form_id, "depth": 0 })) {
            Ok(value) => value["editor_id"].as_str().unwrap_or_default().to_owned(),
            Err(_) => String::new(),
        }
    }

    /// The model prefixes of the idle records of a file, which the session
    /// has no command for: the shell reads them.
    fn idle_prefixes(&mut self, file: &str) -> Vec<String> {
        let records = match self.call_session(
            "records.list",
            json!({ "file": file, "signature": "IDLE", "limit": null }),
        ) {
            Ok(value) => value["records"].as_array().cloned().unwrap_or_default(),
            Err(error) => {
                self.add_message(&format!("Fatal: {}", error.message));
                return Vec::new();
            }
        };
        let mut prefixes: Vec<String> = Vec::new();
        for record in records {
            let Some(form_id) = record["form_id"].as_str() else {
                continue;
            };
            let Ok(value) = self.call_session("records.get", json!({ "form_id": form_id, "file": file, "depth": 1 }))
            else {
                continue;
            };
            let Some(model) = find_element_value(&value, "MODL", "MODL") else {
                continue;
            };
            let model = model.trim().to_ascii_lowercase();
            let prefix = if model.rfind('.').is_some() {
                model
                    .rsplit_once('\\')
                    .map(|(folder, _)| folder)
                    .unwrap_or_default()
                    .to_owned()
            } else {
                model
            };
            let prefix = prefix.trim_end_matches('\\').to_owned();
            if !prefix.is_empty() && !prefixes.contains(&prefix) {
                prefixes.push(prefix);
            }
        }
        prefixes.sort();
        prefixes
    }

    /// The answers of a flow, in the order the asks were answered.
    fn nav_answers(&self) -> Vec<NavAnswer> {
        self.nav_flow
            .as_ref()
            .map(|state| state.answers.clone())
            .unwrap_or_default()
    }

    /// Whether a flow is waiting for an answer. The dialogs the flow
    /// shares with the other menu items (`TfrmFileSelect`,
    /// `TfrmModGroupSelect`) answer the flow only while it waits.
    fn nav_flow_waiting(&self) -> bool {
        self.nav_flow.is_some()
    }

    /// Whether the flow is at a `Files` ask, which the file select
    /// answers. The file select is opened by the other menu items too,
    /// which do not belong to a flow.
    fn nav_flow_awaits_files(&self) -> bool {
        self.nav_flow_waiting()
            && self
                .nav_flow
                .as_ref()
                .and_then(|state| state.asks.get(state.index))
                .is_some_and(|ask| matches!(ask, NavAsk::Files { .. }))
    }

    /// Whether the flow is at a `ModGroups` ask, which the mod group
    /// select answers.
    fn nav_flow_awaits_mod_groups(&self) -> bool {
        self.nav_flow_waiting()
            && self
                .nav_flow
                .as_ref()
                .and_then(|state| state.asks.get(state.index))
                .is_some_and(|ask| matches!(ask, NavAsk::ModGroups { .. }))
    }

    /// The answer of the ask the flow is at, and the next ask opens.
    fn nav_flow_answer(&mut self, answer: NavAnswer) {
        let mut copy_idle = None;
        if let Some(state) = self.nav_flow.as_mut() {
            state.answers.push(answer);
            state.index += 1;
            if state.index == 1 && matches!(state.flow, NavFlow::CopyIdle { .. }) {
                copy_idle = Some(match state.answers.first() {
                    Some(NavAnswer::Files(prefixes)) => prefixes.clone(),
                    _ => Vec::new(),
                });
            }
        }
        if let Some(prefixes) = copy_idle {
            let flow = self.nav_flow.as_ref().map(|state| state.flow.clone());
            if let Some(flow) = flow {
                let asks = self.nav_copy_idle_asks(&flow, &prefixes);
                if let Some(state) = self.nav_flow.as_mut() {
                    state.asks = asks;
                }
            }
        }
        self.advance_nav_flow();
    }

    /// The asks of `CopyIdle` once the prefixes are chosen: one "Model
    /// Prefix" box per chosen prefix, then the files the idles are copied
    /// into.
    fn nav_copy_idle_asks(&mut self, flow: &NavFlow, prefixes: &[String]) -> Vec<NavAsk> {
        let mut asks = vec![NavAsk::Files {
            caption: "Which Idles do you want to copy?".to_owned(),
            items: prefixes.to_vec(),
            checked: Vec::new(),
            single: false,
        }];
        for prefix in prefixes {
            asks.push(NavAsk::Text {
                caption: "Model Prefix".to_owned(),
                message: "Please change the model prefix".to_owned(),
                value: prefix.clone(),
                require_text: true,
            });
        }
        let file = match flow {
            NavFlow::CopyIdle { file } => file.clone(),
            _ => String::new(),
        };
        let modules = self.nav_modules();
        asks.push(NavAsk::Files {
            caption: "Which files do you want to add these records to?".to_owned(),
            items: copy_target_candidates(&modules, &file),
            checked: Vec::new(),
            single: false,
        });
        asks
    }

    /// Whether a path names one of the loaded modules.
    fn is_module_file(&self, path: &str) -> bool {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.files.iter().any(|file| file.eq_ignore_ascii_case(&name))
    }

    /// Whether the flow is the delta patch and waits for the name of the
    /// patch.
    fn nav_flow_awaits_delta_patch_name(&self) -> bool {
        let Some(state) = self.nav_flow.as_ref() else {
            return false;
        };
        matches!(state.flow, NavFlow::DeltaPatch { .. }) && state.index == 1
    }

    /// The path of a file of the data folder.
    fn data_file(&self, name: &str) -> String {
        let separator = if self.data_path.ends_with(['\\', '/']) {
            ""
        } else {
            "\\"
        };
        format!("{}{separator}{name}", self.data_path)
    }

    /// Whether the name of a delta patch is taken: the module of that name
    /// is in the data folder already.
    fn nav_delta_patch_name_taken(&self, name: &str) -> bool {
        if !self.nav_flow_awaits_delta_patch_name() {
            return false;
        }
        std::path::Path::new(&self.data_file(&format!("{name}.esu"))).exists()
    }

    /// A cancelled dialog ends the flow.
    fn nav_flow_cancel(&mut self) {
        self.nav_flow = None;
        self.nav_question = None;
    }

    /// The names of the first `Files` answer of the flow.
    fn nav_files(&self, index: usize) -> Vec<String> {
        match self.nav_answers().get(index) {
            Some(NavAnswer::Files(items)) => items.clone(),
            _ => Vec::new(),
        }
    }

    /// Runs the command of the flow with the answers.
    fn finish_nav_flow(&mut self) {
        let Some(state) = self.nav_flow.take() else {
            return;
        };
        match state.flow {
            NavFlow::CompareTo { file } => self.run_nav_compare_to(&file, &state.answers),
            NavFlow::DeltaPatch { file } => self.run_nav_delta_patch(&file, &state.answers),
            NavFlow::BatchChange { file } => self.run_nav_batch_change(&file, &state.answers),
            NavFlow::AddMasters { file } => self.run_nav_add_masters(&file, &state.answers),
            NavFlow::BuildRef => self.run_nav_build_ref(&state.answers),
            NavFlow::ChangeFormId { form_id, file } => {
                self.run_nav_change_form_id(&form_id, &file, None, 0, &state.answers)
            }
            NavFlow::ChangeFormIdInto { form_id, file } => {
                let target = self.nav_files(0).into_iter().next();
                self.run_nav_change_form_id(&form_id, &file, target, 1, &state.answers)
            }
            NavFlow::ChangeReferencingRecords { form_id, file } => {
                self.run_nav_change_referencing_records(&form_id, &file, &state.answers)
            }
            NavFlow::RenumberFrom { file } => self.run_nav_renumber(&file, None, &state.answers),
            NavFlow::RenumberInject { file } => {
                let target = self.nav_files(0).into_iter().next();
                self.run_nav_renumber(&file, target, &state.answers)
            }
            NavFlow::SetVwdInto { records } => self.run_nav_set_vwd_into(&records, &state.answers),
            NavFlow::UpdateCrc => self.run_nav_update_crc(&state.answers),
            NavFlow::CreateModGroup { .. } => self.run_nav_create_mod_group(&state.answers),
            NavFlow::DeleteModGroups => self.run_nav_delete_mod_groups(&state.answers),
            NavFlow::CopyIdle { file } => self.run_nav_copy_idle(&file, &state.answers),
            NavFlow::MergedPatch { file } => self.run_nav_merged_patch(&file, &state.answers),
            NavFlow::NewFile => self.run_nav_new_file(&state.answers),
            NavFlow::CopyInto { kind, records } => self.run_nav_copy_into(kind, &records, &state.answers),
            NavFlow::FilterApply { files, conflicts } => self.run_nav_filter_apply(&files, conflicts, &state.answers),
        }
    }

    /// The EditorID boxes of the copy flow are asked again: the flow
    /// starts over with the boxes and the answers it holds are dropped
    /// (`CopyInto`'s `repeat ... until False`, xeMainForm.pas:3486-3506).
    fn copy_repeat_asks(&mut self) {
        let Some(flow) = self.nav_flow.as_ref().map(|state| state.flow.clone()) else {
            return;
        };
        let asks = self.nav_flow_asks(&flow);
        if let Some(state) = self.nav_flow.as_mut() {
            state.asks = asks;
            state.answers.clear();
            state.index = 0;
        }
        self.advance_nav_flow();
    }

    /// `mniNavCompareToClick`: the chosen file is copied into the folder of
    /// the focused plugin and loaded in its place, at its load order, as a
    /// compare load (`files.compare_to`).
    fn run_nav_compare_to(&mut self, file: &str, answers: &[NavAnswer]) {
        let Some(NavAnswer::Path(chosen)) = answers.first() else {
            return;
        };
        match self.call_session(
            "files.compare_to",
            json!({
                "file": file,
                "path": chosen,
                "dry_run": false,
            }),
        ) {
            Ok(_value) => {
                self.add_message(&format!("Comparing {} with the chosen file", file));
                // The compare file replaces the plugin in the tree, as
                // upstream's loader thread does (`AddFile`).
                self.nav = NavSource::from_loaded();
                self.nav_state.selected.clear();
                self.nav_state.focused = None;
                self.nav_state.anchor = None;
                self.refresh_session_files();
            }
            Err(error) => self.add_message(&format!("files.compare_to: {}", error.message)),
        }
    }

    /// The files of the session, as `session.info` reports them.
    fn refresh_session_files(&mut self) {
        if let Ok(value) = self.call_session("session.info", json!({})) {
            if let Some(files) = value["files"].as_array() {
                self.files = files
                    .iter()
                    .filter_map(|file| file.as_str().map(str::to_owned))
                    .collect();
            }
            if let Some(ids) = value["file_ids"].as_array() {
                self.file_ids = ids.iter().filter_map(|id| id.as_i64().map(|id| id as i32)).collect();
            }
        }
    }

    /// `mniNavCreateDeltaPatchClick`: the newer version of the module and
    /// the name of the patch.
    fn run_nav_delta_patch(&mut self, file: &str, answers: &[NavAnswer]) {
        let Some(NavAnswer::Path(newer)) = answers.first() else {
            return;
        };
        let name = match answers.get(1) {
            Some(NavAnswer::Text(name)) => name.clone(),
            _ => String::new(),
        };
        self.run_session_command(
            "files.create_delta_patch",
            json!({
                "file": file,
                "newer": newer,
                "name": name,
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavBatchChangeReferencingRecordsClick`: the CSV file and the
    /// pairs it names.
    fn run_nav_batch_change(&mut self, file: &str, answers: &[NavAnswer]) {
        let Some(NavAnswer::Path(path)) = answers.first() else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            self.add_message(&format!("Could not read {path}"));
            return;
        };
        let (lines, messages) = parse_batch_csv(&text);
        for message in messages {
            self.add_message(&message);
        }
        let pairs: Vec<xedit_session::refs::ReferencePair> = batch_pairs(&lines);
        let pairs: Vec<serde_json::Value> = pairs
            .iter()
            .map(|pair| {
                json!({
                    "old_form_id": pair.old_form_id,
                    "old_file": pair.old_file,
                    "new_form_id": pair.new_form_id,
                    "new_file": pair.new_file,
                })
            })
            .collect();
        self.run_session_command(
            "references.change_batch",
            json!({
                "file": file,
                "pairs": pairs,
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavAddMastersClick`: the masters the user checked.
    fn run_nav_add_masters(&mut self, file: &str, answers: &[NavAnswer]) {
        let Some(NavAnswer::Files(masters)) = answers.first() else {
            return;
        };
        if masters.is_empty() {
            return;
        }
        // Upstream asks the edit warning after the selection; the port
        // asks it through the same `edit_warn_ok` seam the other items
        // use.
        if !self.edit_warn_ok {
            self.pending_masters = Some((file.to_owned(), masters.clone()));
            self.open_edit_warning();
            return;
        }
        self.run_masters_add(file, masters);
    }

    /// `masters.add` with the masters the user checked.
    fn run_masters_add(&mut self, file: &str, masters: &[String]) {
        self.run_session_command(
            "masters.add",
            json!({
                "file": file,
                "masters": masters,
                "sort": true,
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavBuildRefClick`: the reference information of the files the
    /// user checked.
    fn run_nav_build_ref(&mut self, answers: &[NavAnswer]) {
        let Some(NavAnswer::Files(files)) = answers.first() else {
            return;
        };
        if files.is_empty() {
            self.add_message("There are no files without reference information");
            return;
        }
        for file in files {
            self.run_session_command(
                "refs.build",
                json!({
                    "file": file,
                    "only_load": false,
                }),
                None,
            );
        }
    }

    /// `mniNavChangeFormIDClick`: the new FormID of the record and the
    /// question about its later overrides.
    fn run_nav_change_form_id(
        &mut self,
        form_id: &str,
        file: &str,
        target: Option<String>,
        text_index: usize,
        answers: &[NavAnswer],
    ) {
        let new_form_id = match answers.get(text_index) {
            Some(NavAnswer::Text(text)) => text.clone(),
            _ => String::new(),
        };
        if new_form_id.trim().is_empty() {
            self.add_message("00000000 is not a valid FormID");
            return;
        }
        // UPSTREAM-QUIRK: upstream asks the question only when the record
        // has later overrides, which no session command reports. The port
        // asks it always and `formids.change` ignores the answer when
        // there is nothing to update.
        let overrides = answers
            .get(text_index + 1)
            .is_some_and(|answer| matches!(answer, NavAnswer::Yes(true)));
        self.run_session_command(
            "formids.change",
            json!({
                "form_id": form_id,
                "new_form_id": new_form_id,
                "file": file,
                "target_file": target,
                "overrides": overrides,
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavChangeReferencingRecordsClick`: the new FormID the
    /// referencing records get.
    ///
    /// UPSTREAM-QUIRK: upstream asks which referencing records to update
    /// (`ShowChangeReferencedBy`); the port updates all of them, because
    /// no session command lists them.
    fn run_nav_change_referencing_records(&mut self, form_id: &str, file: &str, answers: &[NavAnswer]) {
        let new_form_id = match answers.first() {
            Some(NavAnswer::Text(text)) => text.clone(),
            _ => String::new(),
        };
        self.run_session_command(
            "references.change",
            json!({
                "form_id": form_id,
                "file": file,
                "new_form_id": new_form_id,
                "files": [],
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavRenumberFormIDsFromClick` and `mniNavRenumberFormIDsInjectClick`:
    /// the start FormID and the master the records are injected into.
    ///
    /// UPSTREAM-QUIRK: upstream asks the two ObjectID questions and the
    /// final warning, and it refuses when the treeview carries a filter.
    /// The port runs the command with the options the command understands;
    /// the questions are not asked, because the command decides what it
    /// preserves.
    fn run_nav_renumber(&mut self, file: &str, target: Option<String>, answers: &[NavAnswer]) {
        let start = match answers.last() {
            Some(NavAnswer::Text(text)) => text.clone(),
            _ => String::new(),
        };
        self.run_session_command(
            "formids.renumber",
            json!({
                "file": file,
                "start": start,
                "compact": false,
                "inject_into": target,
                "preserve_object_ids": false,
                "all_or_nothing": false,
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavSetVWDAutoIntoClick`: the files the references are copied
    /// into.
    fn run_nav_set_vwd_into(&mut self, records: &[(String, String)], answers: &[NavAnswer]) {
        let Some(NavAnswer::Files(targets)) = answers.first() else {
            return;
        };
        let Some(target) = targets.first() else {
            return;
        };
        let form_ids: Vec<String> = records.iter().map(|(form_id, _)| form_id.clone()).collect();
        self.run_session_command(
            "references.copy_vwd_auto",
            json!({
                "files": [],
                "records": form_ids,
                "to": target,
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavUpdateCRCModGroupsClick`: the two questions, the modules and
    /// the mod groups.
    fn run_nav_update_crc(&mut self, answers: &[NavAnswer]) {
        let add = answers
            .first()
            .is_some_and(|answer| matches!(answer, NavAnswer::Yes(true)));
        let update = answers
            .get(1)
            .is_some_and(|answer| matches!(answer, NavAnswer::Yes(true)));
        if !add && !update {
            return;
        }
        let modules = match answers.get(2) {
            Some(NavAnswer::Files(modules)) => modules.clone(),
            _ => Vec::new(),
        };
        let mod_groups = match answers.get(3) {
            Some(NavAnswer::ModGroups(groups)) => groups.clone(),
            _ => Vec::new(),
        };
        self.run_session_command(
            "modgroups.update_crcs",
            json!({
                "add": add,
                "update": update,
                "modules": modules,
                "mod_groups": mod_groups,
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavCreateModGroupClick`: the name of the group, its modules and
    /// the module whose `.modgroups` file gets it.
    fn run_nav_create_mod_group(&mut self, answers: &[NavAnswer]) {
        let name = match answers.first() {
            Some(NavAnswer::Text(name)) => name.clone(),
            _ => String::new(),
        };
        let modules = match answers.get(1) {
            Some(NavAnswer::Files(modules)) => modules.clone(),
            _ => Vec::new(),
        };
        let file = match answers.get(2) {
            Some(NavAnswer::Files(files)) => files.first().cloned(),
            _ => None,
        };
        self.run_session_command(
            "modgroups.create",
            json!({
                "name": name,
                "modules": modules,
                "items": [],
                "include_crcs": false,
                "add_current_crcs": false,
                "file": file,
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavDeleteModGroupsClick`: the groups the user checked.
    fn run_nav_delete_mod_groups(&mut self, answers: &[NavAnswer]) {
        let Some(NavAnswer::ModGroups(groups)) = answers.first() else {
            return;
        };
        if groups.is_empty() {
            return;
        }
        self.run_session_command(
            "modgroups.delete",
            json!({
                "names": groups,
                "file": null,
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavCopyIdleClick`: the model prefix and the file the idles are
    /// copied into.
    fn run_nav_copy_idle(&mut self, _file: &str, answers: &[NavAnswer]) {
        let Some(NavAnswer::Files(prefixes)) = answers.first() else {
            return;
        };
        if prefixes.is_empty() {
            return;
        }
        // One "Model Prefix" box per chosen prefix, then the list of the
        // files the idles are copied into.
        let Some(NavAnswer::Files(targets)) = answers.get(prefixes.len() + 1) else {
            return;
        };
        let Some(to) = targets.first() else {
            return;
        };
        for (index, prefix) in prefixes.iter().enumerate() {
            let new_path = match answers.get(index + 1) {
                Some(NavAnswer::Text(text)) => text.clone(),
                _ => String::new(),
            };
            self.run_session_command(
                "records.copy_idles",
                json!({
                    "path": prefix,
                    "new_path": new_path,
                    "to": to,
                    "dry_run": false,
                }),
                None,
            );
        }
    }

    /// `mniNavCreateMergedPatchClick`: the name of the new file.
    ///
    /// UPSTREAM-QUIRK: upstream merges the whole session into the new file,
    /// so the focused file does not matter, and the file stays in the
    /// session for the user to save. The port's `patch.merged` merges the
    /// session too, and its `file` field is the name of the new patch.
    fn run_nav_merged_patch(&mut self, _file: &str, answers: &[NavAnswer]) {
        let name = match answers.first() {
            Some(NavAnswer::Text(name)) => name.clone(),
            _ => String::new(),
        };
        if name.trim().is_empty() {
            return;
        }
        match self.call_session(
            "patch.merged",
            json!({
                "file": name,
                "dry_run": false,
                "save": false,
                "output": null,
                "backup": false,
            }),
        ) {
            Ok(value) => {
                // The command warns about the games whose merged patch is
                // unsupported, as upstream's question does.
                if let Some(warning) = value["warning"].as_str() {
                    self.add_message(warning);
                }
                self.after_command("patch.merged", &value);
            }
            Err(error) => self.add_message(&format!("patch.merged: {}", error.message)),
        }
    }

    /// `mniCreateNewFileClick`: the template of the file and its name.
    fn run_nav_new_file(&mut self, answers: &[NavAnswer]) {
        let template = match answers.first() {
            Some(NavAnswer::Files(templates)) => templates.first().cloned().unwrap_or_default(),
            _ => String::new(),
        };
        let name = match answers.get(1) {
            Some(NavAnswer::Text(name)) => name.clone(),
            _ => String::new(),
        };
        if name.trim().is_empty() {
            return;
        }
        self.run_session_command(
            "files.new",
            json!({
                "file": name,
                "light": false,
                "medium": false,
                "template": file_template(&template),
                "dry_run": false,
            }),
            None,
        );
    }

    /// `mniNavCopyIntoClick` and the other copy items: the copy of every
    /// selected record into the file the user chose.
    ///
    /// UPSTREAM-QUIRK: `records.copy` has no field for the editor ID of a
    /// copied record, and its `prefix_remove`/`prefix` fields only apply
    /// to a deep copy. The port sets the editor ID of the copy with
    /// `elements.set` on the copied record, whose editor ID subrecord is
    /// the element `Editor ID`.
    fn run_nav_copy_into(&mut self, kind: NavCopyIntoKind, records: &[(String, String)], answers: &[NavAnswer]) {
        let (as_new, deep, overwrite, as_wrapper, as_spawn_rate) = kind.request();
        let mut text_index = 0;
        let mut editor_id = String::new();
        let mut wrapper_editor_id = None;
        let mut prefix_remove = String::new();
        let mut suffix_remove = String::new();
        let mut prefix = String::new();
        let mut suffix = String::new();
        match kind {
            NavCopyIntoKind::AsNew if records.len() == 1 => {
                editor_id = match answers.first() {
                    Some(NavAnswer::Text(text)) => text.clone(),
                    _ => String::new(),
                };
                text_index = 1;
            }
            NavCopyIntoKind::AsWrapper if records.len() == 1 => {
                wrapper_editor_id = match answers.first() {
                    Some(NavAnswer::Text(text)) => Some(text.clone()),
                    _ => None,
                };
                text_index = 1;
            }
            NavCopyIntoKind::AsNew | NavCopyIntoKind::AsWrapper => {
                // The four prompts: the prefix and the suffix that are
                // removed, then the prefix and the suffix that are added.
                prefix_remove = match answers.first() {
                    Some(NavAnswer::Text(text)) => text.clone(),
                    _ => String::new(),
                };
                suffix_remove = match answers.get(1) {
                    Some(NavAnswer::Text(text)) => text.clone(),
                    _ => String::new(),
                };
                prefix = match answers.get(2) {
                    Some(NavAnswer::Text(text)) => text.clone(),
                    _ => String::new(),
                };
                suffix = match answers.get(3) {
                    Some(NavAnswer::Text(text)) => text.clone(),
                    _ => String::new(),
                };
                text_index = 4;
            }
            _ => {}
        }
        let targets = match answers.get(text_index) {
            Some(NavAnswer::Files(files)) => files.clone(),
            _ => Vec::new(),
        };
        let Some(target) = targets.first() else {
            return;
        };
        for (form_id, file) in records {
            let copied = self.call_session(
                "records.copy",
                json!({
                    "form_id": form_id,
                    "to": target,
                    "from": file,
                    "as_new": as_new,
                    "deep": deep,
                    "overwrite": overwrite,
                    "remove_deleted": false,
                    "prefix_remove": prefix_remove,
                    "suffix_remove": suffix_remove,
                    "prefix": prefix,
                    "suffix": suffix,
                    "as_wrapper": as_wrapper,
                    "wrapper_editor_id": wrapper_editor_id,
                    "as_spawn_rate": as_spawn_rate,
                    "disable": false,
                    "dry_run": false,
                }),
            );
            let Ok(copied) = copied else {
                continue;
            };
            // The editor ID of the copy, which the command has no field
            // for.
            if !editor_id.is_empty() {
                let Some(copy_form_id) = copied["copy"]["form_id"].as_str() else {
                    self.add_message(&format!(
                        "The copy of {form_id} has no FormID of its own: its EditorID was not changed"
                    ));
                    continue;
                };
                let request = ElementsSetRequest {
                    form_id: copy_form_id.to_owned(),
                    path: "Editor ID".to_owned(),
                    file: Some(target.clone()),
                    value: serde_json::Value::String(editor_id.clone()),
                    dry_run: false,
                };
                if let Err(error) =
                    self.call_session("elements.set", serde_json::to_value(&request).unwrap_or_default())
                {
                    self.add_message(&format!("elements.set: {}", error.message));
                }
            }
        }
    }

    /// `mniNavFilterApplySelected` and `mniNavFilterConflictsSelected`: the
    /// files the filter runs over.
    fn run_nav_filter_apply(&mut self, files: &[String], conflicts: bool, answers: &[NavAnswer]) {
        let selected = match answers.first() {
            Some(NavAnswer::Files(files)) => files.clone(),
            _ => files.to_vec(),
        };
        let mut request = xedit_session::filter::FilterApplyRequest {
            files: selected,
            options: None,
            preset: None,
            mod_groups: Vec::new(),
            all_mod_groups: false,
            saved_mod_groups: false,
            list_records: false,
            conflict_only: false,
            only_one: false,
            no_game_master: false,
        };
        if conflicts {
            // `mniNavFilterConflictsClick`: the conflict statuses
            // `[ctIdenticalToMasterWinsConflict, ctConflictWins,
            // ctConflictLoses]`, the flattening and the inheritance on.
            request.options = Some(xedit_session::filter::FilterRequestOptions {
                conflict_this: Some(vec![
                    xedit_session::conflicts::ConflictThisName::ctIdenticalToMasterWinsConflict,
                    xedit_session::conflicts::ConflictThisName::ctConflictWins,
                    xedit_session::conflicts::ConflictThisName::ctConflictLoses,
                ]),
                flatten_blocks: true,
                flatten_cell_childs: true,
                assign_pers_wrld_child: true,
                inherit_conflict_by_parent: true,
                ..Default::default()
            });
        }
        let params = serde_json::to_value(&request).unwrap_or_default();
        self.start_job(
            "filter.apply",
            params,
            JobSpec {
                caption: "Filtering".to_owned(),
                message: "Applying the filter for cleaning. Please Wait...".to_owned(),
                can_cancel: true,
                show_delay: 200,
                update_delay: 100,
            },
            PendingJob::Filter { rect: None },
        );
    }

    /// Opens the dialog of a nav item that has no flow of its own.
    fn open_nav_dialog(&mut self, dialog: NavDialog) {
        match dialog {
            NavDialog::FilterOptions => self.open_filter_options(),
            NavDialog::Script => self.open_dialog(DialogKind::Script),
            NavDialog::Lodgen => self.open_lodgen(),
            NavDialog::LogAnalyzer => self.open_dialog(DialogKind::LogAnalyzer),
            NavDialog::LocalizePlugin => self.open_dialog(DialogKind::LocalizePlugin),
            NavDialog::Options => self.open_options(),
            NavDialog::EditModGroup => self.open_mod_group_select(GroupSelectFlow::Edit),
        }
    }

    /// The yes/no question of a flow, drawn as a modal window of its own
    /// (`MessageDlg`), so it never competes with the dialog of the next
    /// ask.
    fn nav_flow_question_window(&mut self, ctx: &egui::Context) {
        let Some(question) = self.nav_question.take() else {
            return;
        };
        let mut answer = None;
        egui::Window::new("xEdit")
            .id(egui::Id::unique("nav-flow-question"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(&question.message);
                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("No").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if answer.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            // A closed question takes its default answer.
            answer = Some(question.default_yes);
        }
        match answer {
            Some(yes) => {
                if self.nav_question_is_copy {
                    // The copy flow's warning of an unchanged EditorID
                    // (`CopyInto`, xeMainForm.pas:3499-3506): the copy
                    // continues when the user confirms it, and the boxes
                    // are asked again when they do not.
                    self.nav_question_is_copy = false;
                    if !yes {
                        // The boxes are asked again (`copy_repeat_asks`
                        // continues the flow itself).
                        self.copy_repeat_asks();
                        return;
                    }
                    if let Some(state) = self.nav_flow.as_mut() {
                        state.answers.push(NavAnswer::Yes(yes));
                        state.index += 1;
                    }
                    self.advance_nav_flow();
                    return;
                }
                if let Some(state) = self.nav_flow.as_mut() {
                    state.answers.push(NavAnswer::Yes(yes));
                    state.index += 1;
                }
                self.advance_nav_flow();
            }
            None => self.nav_question = Some(question),
        }
    }

    /// The `MessageDlg` of a queued confirmation, drawn as a modal window.
    fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &mut self.confirm else {
            return;
        };
        let mut answer = None;
        egui::Window::new("xEdit")
            .id(egui::Id::unique("confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(&confirm.message);
                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("No").clicked() {
                        answer = Some(false);
                    }
                });
            });
        match answer {
            Some(true) => {
                let confirm = self.confirm.take().expect("checked above");
                self.run_nav_actions(ctx, confirm.actions);
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }

    /// The conflict statuses of the rows the tree just painted, asked for in
    /// one `conflicts.list` request per frame.
    fn load_visible_conflicts(&mut self, ctx: &egui::Context) {
        let pending = self.nav.pending_conflicts();
        if pending.is_empty() {
            return;
        }
        let records: Vec<Value> = pending
            .iter()
            .map(|(form_id, file)| json!({ "form_id": form_id, "file": file }))
            .collect();
        let full = pending.len() >= crate::nav_tree::PENDING_CONFLICT_BATCH;
        match self.call_session("conflicts.list", json!({ "records": records, "limit": null })) {
            Ok(value) => {
                if let Ok(response) = serde_json::from_value::<xedit_session::conflicts::ConflictsResponse>(value) {
                    let statuses: Vec<(String, String, ConflictAll, ConflictThis)> = response
                        .records
                        .iter()
                        .map(|record| {
                            (
                                record.file.clone(),
                                record.form_id.clone(),
                                ConflictAll::from(record.conflict_all),
                                ConflictThis::from(record.conflict_this),
                            )
                        })
                        .collect();
                    self.nav.apply_conflicts(&statuses);
                }
            }
            Err(error) => self.add_message(&format!("conflicts.list: {}", error.message)),
        }
        if full {
            ctx.request_repaint();
        }
    }

    /// The game selection dialog of the startup flow.
    fn game_select_screen(&mut self, ctx: &egui::Context) {
        let chosen = match &mut self.startup {
            Startup::GameSelect(state) => {
                crate::dialogs::game_select_form::show(ctx, state, &mut self.game_select_error)
            }
            _ => return,
        };
        match chosen {
            Some(crate::dialogs::game_select_form::GameSelectAction::Chosen(game)) => {
                self.choose_game(game);
                self.open_module_select(game);
            }
            Some(crate::dialogs::game_select_form::GameSelectAction::Cancelled) => {
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
    }

    /// The module selection dialog of `DoInit`'s source selection.
    fn module_select_screen(&mut self, ctx: &egui::Context) {
        let action = match &mut self.startup {
            Startup::ModuleSelect(state) => module_select_form::show(ctx, state),
            _ => return,
        };
        match action {
            Some(ModuleSelectAction::Ok) => {
                if let Startup::ModuleSelect(state) = &mut self.startup {
                    let rect = state.rect;
                    match state.load_order() {
                        Ok(plugins) if !plugins.is_empty() => {
                            if let Some(rect) = rect
                                && let Some(bridge) = self.module_bridge.as_mut()
                            {
                                write_placement(bridge, "frmModuleSelect", rect);
                            }
                            self.start_load(plugins);
                        }
                        Ok(_) => state.error = Some("No modules selected".to_owned()),
                        Err(message) => state.error = Some(message),
                    }
                }
            }
            Some(ModuleSelectAction::Cancelled) => {
                if let Startup::ModuleSelect(state) = &self.startup
                    && let Some(rect) = state.rect
                    && let Some(bridge) = self.module_bridge.as_mut()
                {
                    write_placement(bridge, "frmModuleSelect", rect);
                }
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(action) => self.run_module_action(action),
            None => {}
        }
    }

    /// Runs one module-dialog action. No command runs inside the dialog.
    fn run_module_action(&mut self, action: ModuleSelectAction) {
        match action {
            ModuleSelectAction::PresetLoad(preset) => {
                if preset == "<plugins.txt>" {
                    if let Startup::ModuleSelect(state) = &mut self.startup {
                        state.apply_plugins_txt_preset();
                    }
                    self.refresh_module_presets();
                    return;
                }
                let result = self.module_command(|bridge| {
                    bridge.request::<_, ModulePresetLoadResponse>(
                        "modules.preset_load",
                        &ModulePresetLoadRequest { preset: preset.clone() },
                    )
                });
                match result {
                    Ok(response) => {
                        if let Startup::ModuleSelect(state) = &mut self.startup {
                            state.apply_preset_load(&preset, &response.modules);
                        }
                        self.refresh_module_presets();
                    }
                    Err(error) => self.module_dialog_error(error.message),
                }
            }
            ModuleSelectAction::PresetSave { preset, after_modal } => {
                let modules = match &self.startup {
                    Startup::ModuleSelect(state) => state.checked_names(),
                    _ => Vec::new(),
                };
                let result = self.module_command(|bridge| {
                    bridge.request::<_, xedit_session::module_select::PresetSaveResponse>(
                        "modules.preset_save",
                        &ModulePresetSaveRequest {
                            preset: preset.clone(),
                            modules: Some(modules.clone()),
                            after_modal,
                            confirm: Some(true),
                            dry_run: false,
                        },
                    )
                });
                match result {
                    Ok(_) => {
                        self.refresh_module_presets();
                        if after_modal {
                            self.finish_module_ok();
                        }
                    }
                    Err(error) => self.module_dialog_error(error.message),
                }
            }
            ModuleSelectAction::PresetDelete(preset) => {
                let result = self.module_command(|bridge| {
                    bridge.request::<_, xedit_session::module_select::PresetDeleteResponse>(
                        "modules.preset_delete",
                        &ModulePresetDeleteRequest {
                            preset: preset.clone(),
                            confirm: Some(true),
                            dry_run: false,
                        },
                    )
                });
                match result {
                    Ok(_) => self.refresh_module_presets(),
                    Err(error) => self.module_dialog_error(error.message),
                }
            }
            ModuleSelectAction::AskChangedPreset(preset) => {
                // `if (Result = mrOk) and (LoadedPreset <> '') and
                // IsPresetDifferent(LoadedPreset) then PresetSave(...,
                // True)`: the dry run answers the question without writing.
                let modules = match &self.startup {
                    Startup::ModuleSelect(state) => state.checked_names(),
                    _ => Vec::new(),
                };
                let changed = self
                    .module_command(|bridge| {
                        bridge.request::<_, xedit_session::module_select::PresetSaveResponse>(
                            "modules.preset_save",
                            &ModulePresetSaveRequest {
                                preset: preset.clone(),
                                modules: Some(modules.clone()),
                                after_modal: true,
                                confirm: Some(true),
                                dry_run: true,
                            },
                        )
                    })
                    .map(|response| response.changed)
                    .unwrap_or(false);
                if changed {
                    if let Startup::ModuleSelect(state) = &mut self.startup {
                        state.question = Some(crate::startup::ModuleSelectQuestion::ChangedPreset(preset));
                    }
                } else {
                    self.finish_module_ok();
                }
            }
            _ => {}
        }
    }

    /// One command on the pre-load bridge, with the borrow confined to the
    /// call.
    fn module_command<T>(
        &mut self,
        call: impl FnOnce(&mut CommandBridge) -> Result<T, CommandError>,
    ) -> Result<T, CommandError> {
        match self.module_bridge.as_mut() {
            Some(bridge) => call(bridge),
            None => Err(CommandError::new(
                "no_session",
                "the settings file is not open until a game is chosen",
            )),
        }
    }

    /// The OK of the module dialog after its question was answered.
    fn finish_module_ok(&mut self) {
        if let Startup::ModuleSelect(state) = &mut self.startup {
            match state.load_order() {
                Ok(plugins) if !plugins.is_empty() => self.start_load(plugins),
                Ok(_) => state.error = Some("No modules selected".to_owned()),
                Err(message) => state.error = Some(message),
            }
        }
    }

    fn module_dialog_error(&mut self, message: String) {
        if let Startup::ModuleSelect(state) = &mut self.startup {
            state.error = Some(message);
        }
    }

    /// `PresetsUpdate` after a preset command.
    fn refresh_module_presets(&mut self) {
        let response = match self.module_bridge.as_mut() {
            Some(bridge) => bridge
                .request::<_, ModulePresetsResponse>("modules.presets", &ModulePresetsRequest {})
                .ok(),
            None => None,
        };
        if let (Some(response), Startup::ModuleSelect(state)) = (response, &mut self.startup) {
            state.set_presets(response.presets);
        }
    }

    /// `wbModulesByLoadOrder` for `TfrmModuleSelect`, through the command:
    /// `selectable_modules` primes the game paths and settings the command
    /// reads, then a session over no files serves it.
    pub fn open_module_select(&mut self, game: GameMode) {
        let tag = xedit_session::dump::game_tag(game);
        match xedit_session::module_select::selectable_modules(tag) {
            Ok(_) => {
                let mut session = Session::with_files(game, Vec::new());
                session.allow_edit(self.edit_allowed);
                let mut bridge = CommandBridge::new(session);
                let request = ModulesListRequest {
                    valid: true,
                    active: false,
                    masters_present: false,
                };
                match bridge.request::<_, ModulesListResponse>("modules.list", &request) {
                    Ok(list) => {
                        let mut state = ModuleSelectState::from_modules_list(list, ModuleSelectMode::Active, &[]);
                        state.placement = read_placement(&mut bridge, "frmModuleSelect");
                        if let Ok(presets) =
                            bridge.request::<_, ModulePresetsResponse>("modules.presets", &ModulePresetsRequest {})
                        {
                            state.set_presets(presets.presets);
                        }
                        self.module_bridge = Some(bridge);
                        self.startup = Startup::ModuleSelect(Box::new(state));
                    }
                    Err(error) => {
                        self.startup = Startup::Failed(format!("{}: {}", error.code, error.message));
                    }
                }
            }
            Err(error) => {
                self.startup = Startup::Failed(format!("{}: {}", error.code, error.message));
            }
        }
    }

    /// Writes the placement of a loaded-session dialog (`[frmX]` keys).
    fn save_dialog_placement(&mut self, section: &str, rect: Option<egui::Rect>) {
        if let Some(rect) = rect
            && let Some(bridge) = self.bridge.as_mut()
        {
            write_placement(bridge, section, rect);
        }
    }

    /// The failure screen: the `Fatal:` message of a load.
    fn failure_screen(&mut self, ctx: &egui::Context) {
        let Startup::Failed(message) = &self.startup else {
            return;
        };
        let mut retry = false;
        let mut quit = false;
        egui::Window::new("xEdit")
            .id(egui::Id::unique("fatal"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.colored_label(ui.visuals().error_fg_color, message);
                ui.horizontal(|ui| {
                    retry = ui.button("Select a game again").clicked();
                    quit = ui.button("Quit").clicked();
                });
            });
        if retry {
            self.startup = Startup::game_select();
            self.game = None;
        }
        if quit {
            self.closing = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Which handler opened `TfrmModGroupSelect`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSelectFlow {
    /// `mniViewModGroupsReloadClick`: the valid groups, the saved
    /// selection checked, the presets shown.
    Reload,
    /// `mniNavEditModGroupClick`: every group, none checked, one at most,
    /// then the edit form.
    Edit,
}

impl TfrmMain {
    /// One command on the loaded session, with the borrow confined to the
    /// call.
    fn dialog_command<T>(
        &mut self,
        call: impl FnOnce(&mut CommandBridge) -> Result<T, CommandError>,
    ) -> Result<T, CommandError> {
        match self.bridge.as_mut() {
            Some(bridge) => call(bridge),
            None => Err(CommandError::new("no_session", "no session is loaded")),
        }
    }

    /// The keys and values of a settings section, for the preset values.
    fn settings_values(&mut self, section: &str) -> Vec<(String, String)> {
        let request = SettingsGetRequest {
            section: Some(section.to_owned()),
            key: None,
            value_type: None,
            default: None,
        };
        // The options dialog can open before a load (`mniMainOptions`); the
        // pre-load session serves the settings file the same way.
        let response = match self.bridge.as_mut() {
            Some(bridge) => bridge.request::<_, SettingsGetResponse>("settings.get", &request),
            None => match self.module_bridge.as_mut() {
                Some(bridge) => bridge.request::<_, SettingsGetResponse>("settings.get", &request),
                None => return Vec::new(),
            },
        };
        response
            .map(|response| {
                response
                    .values
                    .into_iter()
                    .map(|entry| (entry.key, entry.raw))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn write_setting(&mut self, section: &str, key: &str, value: &str) {
        let _ = self.dialog_command(|bridge| {
            bridge.request::<_, SettingsSetResponse>(
                "settings.set",
                &SettingsSetRequest {
                    section: section.to_owned(),
                    key: key.to_owned(),
                    value: serde_json::json!(value),
                    value_type: Some("string".to_owned()),
                    dry_run: false,
                },
            )
        });
    }

    /// `--dialog <name>` after the load.
    fn open_pending_dialog(&mut self, name: &str) {
        match name {
            "filter" => self.open_filter_options(),
            "modgroups" => self.open_mod_group_select(GroupSelectFlow::Reload),
            "modgroup-edit" => match self.dialog_arg.clone() {
                Some(group) => self.open_mod_group_edit(&group),
                None => self.open_mod_group_select(GroupSelectFlow::Edit),
            },
            "files" => self.open_file_select(),
            // The modal tools of step 7, which have no menu path the
            // screen-check harness can drive.
            "options" => {
                self.open_options();
                let tab = self.dialog_arg.clone();
                if let (Some(tab), Some(Dialog::Options(state))) = (tab, self.dialog.as_mut()) {
                    state.set_tab_by_name(&tab);
                }
            }
            "lodgen" => self.open_lodgen(),
            "legend" => self.open_legend(),
            "edit-warning" => self.open_edit_warning(),
            "worldspace-cell" => self.prompt_cell_details("Tamriel"),
            "formid" => {
                let suggested = self.dialog_arg.clone().unwrap_or_else(|| "04000000".to_owned());
                self.prompt_form_id(&suggested);
            }
            other => self.add_message(&format!("--dialog {other}: the port has no such dialog")),
        }
    }

    /// `mniNavFilterApplyClick`: `TfrmFilterOptions` with the two signature
    /// lists, the presets and the last used preset.
    fn open_filter_options(&mut self) {
        let presets = match self.dialog_command(|bridge| {
            bridge.request::<_, FilterPresetsResponse>("filter.presets", &FilterPresetsRequest {})
        }) {
            Ok(presets) => presets,
            Err(error) => {
                self.add_message(&format!("Fatal: {}", error.message));
                return;
            }
        };
        let signatures = match self.dialog_command(|bridge| {
            bridge.request::<_, SignaturesListResponse>("signatures.list", &SignaturesListRequest {})
        }) {
            Ok(signatures) => signatures,
            Err(error) => {
                self.add_message(&format!("Fatal: {}", error.message));
                return;
            }
        };
        let last = presets.last_used.clone();
        let values = if last.is_empty() {
            Vec::new()
        } else {
            self.settings_values(&format!("Filter {last}"))
        };
        let reachable = xedit_core::interface::globals::reachable_build();
        let mut state = FilterOptionsState::new(
            &signatures.records,
            &signatures.base_records,
            presets.presets,
            last.clone(),
            reachable,
        );
        state.apply_preset_values(&last, &values);
        if let Some(bridge) = self.bridge.as_mut() {
            state.placement = read_placement(bridge, "frmFilterOptions");
        }
        self.dialog = Some(Dialog::FilterOptions(Box::new(state)));
    }

    /// `TfrmModGroupSelect` of the Reload and Edit handlers.
    fn open_mod_group_select(&mut self, flow: GroupSelectFlow) {
        let list = match self.dialog_command(|bridge| {
            bridge.request::<_, ModGroupListResponse>(
                "modgroups.list",
                &ModGroupListRequest {
                    all: flow == GroupSelectFlow::Edit,
                },
            )
        }) {
            Ok(list) => list,
            Err(error) => {
                self.add_message(&format!("Fatal: {}", error.message));
                return;
            }
        };
        crate::log_screen(
            "modgroups",
            &[format!("{} groups from {:?}", list.mod_groups.len(), list.files)],
        );
        let mut state = match flow {
            GroupSelectFlow::Reload => {
                let presets = match self.dialog_command(|bridge| {
                    bridge.request::<_, ModGroupPresetsResponse>("modgroups.presets", &ModGroupPresetsRequest {})
                }) {
                    Ok(presets) => presets,
                    Err(error) => {
                        self.add_message(&format!("Fatal: {}", error.message));
                        return;
                    }
                };
                ModGroupSelectState::new(
                    list.mod_groups,
                    presets.presets,
                    presets.last,
                    &presets.selection,
                    "Reloading ModGroups - Which ModGroups do you want to activate?",
                    true,
                    0,
                    usize::MAX,
                )
            }
            GroupSelectFlow::Edit => {
                ModGroupSelectState::for_edit(list.mod_groups, "Which ModGroup do you want to edit?")
            }
        };
        if let Some(bridge) = self.bridge.as_mut() {
            state.placement = read_placement(bridge, "frmModGroupSelect");
        }
        self.edit_group_flow = flow == GroupSelectFlow::Edit;
        self.dialog = Some(Dialog::ModGroupSelect(Box::new(state)));
    }

    /// `TfrmModGroupEdit` for one group (`mniNavEditModGroupClick`'s second
    /// dialog).
    pub fn open_mod_group_edit(&mut self, name: &str) {
        let show = match self.dialog_command(|bridge| {
            bridge.request::<_, ModGroupShowResponse>(
                "modgroups.show",
                &ModGroupShowRequest {
                    name: name.to_owned(),
                    file: None,
                },
            )
        }) {
            Ok(show) => show,
            Err(error) => {
                self.add_message(&format!("Fatal: {}", error.message));
                return;
            }
        };
        let modules = match self.dialog_command(|bridge| {
            bridge.request::<_, ModulesListResponse>(
                "modules.list",
                &ModulesListRequest {
                    valid: true,
                    active: false,
                    masters_present: false,
                },
            )
        }) {
            Ok(modules) => modules,
            Err(error) => {
                self.add_message(&format!("Fatal: {}", error.message));
                return;
            }
        };
        let caption = format!(
            "Editing ModGroup \"{}\" from \"{}\"...",
            show.mod_group.name, show.mod_group.file
        );
        let mut state = ModGroupEditState::new(&show, modules, caption);
        if let Some(bridge) = self.bridge.as_mut() {
            state.placement = read_placement(bridge, "frmModGroupEdit");
        }
        self.dialog = Some(Dialog::ModGroupEdit(Box::new(state)));
    }

    /// `TfrmFileSelect` over the loaded files; the save and dump flows fill
    /// the same state with their own items.
    fn open_file_select(&mut self) {
        let mut state = FileSelectState::new("Select Files", self.files.clone());
        if let Some(bridge) = self.bridge.as_mut() {
            state.placement = read_placement(bridge, "frmFileSelect");
        }
        self.dialog = Some(Dialog::FileSelect(Box::new(state)));
    }

    /// Draws the open dialog and runs its action.
    fn draw_dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        match dialog {
            Dialog::ModGroupSelect(mut state) => {
                let action = mod_group_select_form::show(ctx, &mut state);
                match action {
                    Some(ModGroupSelectAction::Ok(groups)) => {
                        if self.nav_flow_awaits_mod_groups() {
                            self.nav_flow_answer(NavAnswer::ModGroups(groups));
                        } else {
                            self.run_mod_group_select_action(*state, ModGroupSelectAction::Ok(groups));
                        }
                    }
                    Some(ModGroupSelectAction::Cancelled) => {
                        if self.nav_flow_awaits_mod_groups() {
                            self.nav_flow_cancel();
                        } else {
                            self.run_mod_group_select_action(*state, ModGroupSelectAction::Cancelled);
                        }
                    }
                    Some(action) => self.run_mod_group_select_action(*state, action),
                    None => self.dialog = Some(Dialog::ModGroupSelect(state)),
                }
            }
            Dialog::ModGroupEdit(mut state) => {
                let action = mod_group_edit_form::show(ctx, &mut state);
                match action {
                    Some(action) => self.run_mod_group_edit_action(*state, action),
                    None => self.dialog = Some(Dialog::ModGroupEdit(state)),
                }
            }
            Dialog::FilterOptions(mut state) => {
                let action = filter_options_form::show(ctx, &mut state);
                match action {
                    Some(action) => self.run_filter_options_action(*state, action),
                    None => self.dialog = Some(Dialog::FilterOptions(state)),
                }
            }
            Dialog::FileSelect(mut state) => {
                let action = file_select_form::show(ctx, &mut state);
                match action {
                    Some(FileSelectAction::Ok { items, backup }) => {
                        if self.nav_flow_awaits_files() {
                            let items = if self.nav_flow_single {
                                items.into_iter().take(1).collect()
                            } else {
                                items
                            };
                            self.nav_flow_answer(NavAnswer::Files(items));
                        } else {
                            self.add_message(&format!(
                                "{} files selected{}",
                                items.len(),
                                if backup { ", backups are made" } else { "" }
                            ));
                        }
                        self.save_dialog_placement("frmFileSelect", state.rect);
                    }
                    Some(FileSelectAction::Cancelled) => {
                        if self.nav_flow_awaits_files() {
                            self.nav_flow_cancel();
                        }
                        self.save_dialog_placement("frmFileSelect", state.rect);
                    }
                    None => self.dialog = Some(Dialog::FileSelect(state)),
                }
            }
            Dialog::Options(mut state) => {
                let action = options_form::show(ctx, &mut state);
                match action {
                    Some(OptionsAction::Ok) => {
                        self.apply_options(*state, ctx);
                    }
                    // `FormClose` with `ModalResult <> mrOk` restores the
                    // colours; the port applies none until OK.
                    Some(OptionsAction::Cancelled) => {}
                    None => self.dialog = Some(Dialog::Options(state)),
                }
            }
            Dialog::Lodgen(mut state) => {
                let action = lodgen_form::show(ctx, &mut state);
                match action {
                    Some(LodgenAction::Ok) => {
                        self.run_lodgen(&state, false);
                    }
                    Some(LodgenAction::SplitTrees) => {
                        self.run_lodgen(&state, true);
                    }
                    Some(LodgenAction::Cancelled) => {}
                    None => self.dialog = Some(Dialog::Lodgen(state)),
                }
            }
            Dialog::Legend(mut state) => {
                let action = legend_form::show(ctx, &mut state);
                match action {
                    Some(LegendAction::Closed) => {
                        self.save_dialog_placement(legend_form::FORM_SECTION, state.rect);
                    }
                    None => self.dialog = Some(Dialog::Legend(state)),
                }
            }
            Dialog::EditWarning(mut state) => {
                // `Timer1` ticks the countdown once a second; the button's
                // caption follows (`FormShow`).
                if state.tick(Instant::now()) {
                    ctx.request_repaint();
                }
                let action = edit_warning_form::show(ctx, &mut state);
                match action {
                    Some(EditWarningAction::Ok) => {
                        // `Inc(EditWarnCount); Settings.WriteInteger('Usage',
                        // 'EditWarnCount', ...)`.
                        self.write_settings(
                            "Usage",
                            "EditWarnCount",
                            serde_json::json!(state.incremented_count()),
                            "integer",
                        );
                        self.edit_warn_ok = true;
                        // The masters of `mniNavAddMastersClick` wait for
                        // the warning.
                        if let Some((file, masters)) = self.pending_masters.take() {
                            self.run_masters_add(&file, &masters);
                        }
                    }
                    Some(EditWarningAction::Cancelled) => {}
                    None => self.dialog = Some(Dialog::EditWarning(state)),
                }
            }
            Dialog::TextPrompt(mut state) => {
                let action = text_prompt::show(ctx, &mut state);
                match action {
                    Some(TextPromptAction::Ok(text)) => {
                        // `mniNavCreateDeltaPatchClick`: the name of the
                        // delta patch must be free.
                        if self.nav_delta_patch_name_taken(&text) {
                            let message = format!("A module called \"{text}.esu\" already exists.");
                            self.add_message(&message);
                            let mut prompt = TextPromptState::new(state.caption.clone(), message, String::new());
                            prompt.require_text = true;
                            self.dialog = Some(Dialog::TextPrompt(Box::new(prompt)));
                        } else {
                            self.nav_flow_answer(NavAnswer::Text(text));
                        }
                    }
                    Some(TextPromptAction::Cancelled) => {
                        self.nav_flow_cancel();
                    }
                    None => self.dialog = Some(Dialog::TextPrompt(state)),
                }
            }
            Dialog::PathPrompt(mut state) => {
                let action = path_prompt::show(ctx, &mut state);
                match action {
                    Some(PathPromptAction::Ok(path)) => {
                        self.nav_flow_answer(NavAnswer::Path(path));
                    }
                    Some(PathPromptAction::Cancelled) => {
                        self.nav_flow_cancel();
                    }
                    None => self.dialog = Some(Dialog::PathPrompt(state)),
                }
            }
        }
    }

    /// The answer of an open prompt goes back to the waiting core call.
    fn draw_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        match prompt {
            PromptState::FormId { pending, mut state } => match form_id_prompt::show(ctx, &mut state) {
                Some(FormIdAction::Ok(text)) => {
                    let answer = prompts::parse_form_id(&text).map_or(PromptAnswer::FormId(None), |form_id| {
                        PromptAnswer::FormId(Some(form_id))
                    });
                    pending.answer(answer);
                }
                Some(FormIdAction::Cancelled) => pending.answer(PromptAnswer::FormId(None)),
                None => self.prompt = Some(PromptState::FormId { pending, state }),
            },
            PromptState::CellDetails {
                pending,
                mut state,
                worldspace,
            } => match worldspace_cell_details_form::show(ctx, &mut state, &worldspace) {
                Some(WorldspaceCellDetailsAction::Ok { persistent, x, y }) => {
                    pending.answer(PromptAnswer::CellDetails(Some((persistent, x, y))));
                }
                Some(WorldspaceCellDetailsAction::Cancelled) => {
                    pending.answer(PromptAnswer::CellDetails(None));
                }
                None => {
                    self.prompt = Some(PromptState::CellDetails {
                        pending,
                        state,
                        worldspace,
                    })
                }
            },
        }
    }

    /// The questions a worker thread asked (`--dialog worldspace-cell` and
    /// `--dialog formid` open the dialogs directly, for the screen check).
    fn poll_prompts(&mut self) {
        if self.prompt.is_some() {
            return;
        }
        let Some(pending) = self.prompts.pop() else {
            return;
        };
        self.prompt = Some(match pending.request.clone() {
            PromptRequest::FormId { suggested } => PromptState::FormId {
                pending,
                state: FormIdState::new(suggested),
            },
            PromptRequest::CellDetails { worldspace } => PromptState::CellDetails {
                pending,
                state: WorldspaceCellDetailsState::default(),
                worldspace,
            },
        });
    }

    /// `--dialog worldspace-cell`: the question of
    /// `wbGetCellDetailsForWorldspaceCallback` without a waiting command.
    pub fn prompt_cell_details(&mut self, worldspace: &str) {
        self.prompts.queue(PromptRequest::CellDetails {
            worldspace: worldspace.to_owned(),
        });
    }

    /// `--dialog formid`: the prompt of `wbGetFormIDCallback`.
    pub fn prompt_form_id(&mut self, suggested: &str) {
        self.prompts.queue(PromptRequest::FormId {
            suggested: suggested.to_owned(),
        });
    }

    /// `mniViewModGroupsReloadClick`/`mniNavEditModGroupClick` after their
    /// selection dialog answered.
    fn run_mod_group_select_action(&mut self, mut state: ModGroupSelectState, action: ModGroupSelectAction) {
        let rect = state.rect;
        match action {
            ModGroupSelectAction::Ok(names) => {
                if self.edit_group_flow {
                    // `mniNavEditModGroupClick` does not save the selection;
                    // it opens the edit form for the chosen group.
                    self.edit_group_flow = false;
                    if let Some(name) = names.first() {
                        self.open_mod_group_edit(name);
                    }
                    self.save_dialog_placement("frmModGroupSelect", rect);
                    return;
                }
                let request = ModGroupSelectRequest {
                    names,
                    saved: false,
                    all: false,
                    dry_run: false,
                };
                match self.dialog_command(|bridge| {
                    bridge.request::<_, xedit_session::modgroups::Reload>("modgroups.select", &request)
                }) {
                    Ok(reload) => {
                        for message in &reload.validation_messages {
                            self.add_message(message);
                        }
                        self.add_message(&format!("{} ModGroups activated", reload.selected.len()));
                        self.save_dialog_placement("frmModGroupSelect", rect);
                    }
                    Err(error) => {
                        state.error = Some(error.message);
                        self.dialog = Some(Dialog::ModGroupSelect(Box::new(state)));
                    }
                }
            }
            ModGroupSelectAction::Cancelled => {
                self.edit_group_flow = false;
                self.save_dialog_placement("frmModGroupSelect", rect);
            }
            ModGroupSelectAction::PresetLoad(preset) => {
                // `PresetLoad` reads the section; `<Last>` is `[ModGroups]
                // Selection`.
                let value = if preset == mod_group_select_form::CS_LAST {
                    self.settings_values("ModGroups")
                        .into_iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case("Selection"))
                        .map(|(_, value)| value)
                        .unwrap_or_default()
                } else {
                    self.settings_values("frmModGroupSelect_ActiveModGroups_Presets")
                        .into_iter()
                        .find(|(key, _)| key == &preset)
                        .map(|(_, value)| value)
                        .unwrap_or_default()
                };
                state.apply_preset(&preset, &value);
                self.dialog = Some(Dialog::ModGroupSelect(Box::new(state)));
            }
            ModGroupSelectAction::PresetSave { preset, after_modal } => {
                // The command writes the saved selection: select the
                // dialog's names first.
                let names = state.checked_groups();
                let _ = self.dialog_command(|bridge| {
                    bridge.request::<_, xedit_session::modgroups::Reload>(
                        "modgroups.select",
                        &ModGroupSelectRequest {
                            names,
                            saved: false,
                            all: false,
                            dry_run: false,
                        },
                    )
                });
                let _ = self.dialog_command(|bridge| {
                    bridge.request::<_, xedit_session::modgroups::PresetSaveResponse>(
                        "modgroups.preset_save",
                        &ModGroupPresetSaveRequest {
                            preset: preset.clone(),
                            after_modal,
                            confirm: Some(true),
                            dry_run: false,
                        },
                    )
                });
                self.refresh_mod_group_presets(&mut state);
                if after_modal {
                    self.add_message("The ModGroup selection was saved");
                    self.save_dialog_placement("frmModGroupSelect", rect);
                } else {
                    self.dialog = Some(Dialog::ModGroupSelect(Box::new(state)));
                }
            }
            ModGroupSelectAction::PresetDelete(preset) => {
                let _ = self.dialog_command(|bridge| {
                    bridge.request::<_, xedit_session::modgroups::PresetDeleteResponse>(
                        "modgroups.preset_delete",
                        &ModGroupPresetDeleteRequest {
                            preset,
                            confirm: Some(true),
                            dry_run: false,
                        },
                    )
                });
                self.refresh_mod_group_presets(&mut state);
                self.dialog = Some(Dialog::ModGroupSelect(Box::new(state)));
            }
        }
    }

    fn refresh_mod_group_presets(&mut self, state: &mut ModGroupSelectState) {
        if let Ok(presets) = self.dialog_command(|bridge| {
            bridge.request::<_, ModGroupPresetsResponse>("modgroups.presets", &ModGroupPresetsRequest {})
        }) {
            state.set_presets(presets.presets, presets.last);
        }
    }

    /// The edit dialog's OK and Cancel.
    fn run_mod_group_edit_action(&mut self, state: ModGroupEditState, action: ModGroupEditAction) {
        let rect = state.rect;
        match action {
            ModGroupEditAction::Ok => {
                let request = ModGroupEditRequest {
                    name: state.original_name.clone(),
                    file: state.file.clone(),
                    new_name: Some(state.name.clone()),
                    items: Some(state.lines()),
                    add_current_crcs: state.add_current_crcs(),
                    dry_run: false,
                };
                match self
                    .dialog_command(|bridge| bridge.request::<_, ModGroupChangeResponse>("modgroups.edit", &request))
                {
                    Ok(response) => {
                        if let Some(message) = &response.message {
                            self.add_message(message);
                        }
                        if let Some(reload) = &response.reload {
                            for message in &reload.validation_messages {
                                self.add_message(message);
                            }
                            self.add_message(&format!(
                                "ModGroup \"{}\" saved to {}",
                                state.name,
                                response.files.first().cloned().unwrap_or_default()
                            ));
                        }
                        self.save_dialog_placement("frmModGroupEdit", rect);
                    }
                    Err(error) => {
                        self.add_message(&format!("Fatal: {}", error.message));
                        self.dialog = Some(Dialog::ModGroupEdit(Box::new(state)));
                    }
                }
            }
            ModGroupEditAction::Cancelled => {
                self.save_dialog_placement("frmModGroupEdit", rect);
            }
        }
    }

    /// `TfrmFilterOptions.FormClose` and `mniNavFilterApplyClick`.
    fn run_filter_options_action(&mut self, mut state: FilterOptionsState, action: FilterOptionsAction) {
        let rect = state.rect;
        match action {
            FilterOptionsAction::LoadPreset(name) => {
                let values = self.settings_values(&format!("Filter {name}"));
                state.apply_preset_values(&name, &values);
                self.dialog = Some(Dialog::FilterOptions(Box::new(state)));
            }
            FilterOptionsAction::SavePreset(name) => {
                self.save_filter_preset(&state, &name);
                self.refresh_filter_presets(&mut state);
                self.dialog = Some(Dialog::FilterOptions(Box::new(state)));
            }
            FilterOptionsAction::DeletePreset(name) => {
                let _ = self.dialog_command(|bridge| {
                    bridge.request::<_, xedit_session::filter::FilterPresetDeleteResponse>(
                        "filter.preset.delete",
                        &FilterPresetDeleteRequest { name, dry_run: false },
                    )
                });
                self.refresh_filter_presets(&mut state);
                self.dialog = Some(Dialog::FilterOptions(Box::new(state)));
            }
            FilterOptionsAction::Commit => {
                // `FormClose`: `View|LastUsedFilter`, then the preset save
                // when the name is empty or the filter changed and the
                // question was answered Yes.
                let last = state.preset.text.clone();
                self.write_setting("View", "LastUsedFilter", &last);
                if last.is_empty() {
                    self.save_filter_preset(&state, "");
                    self.apply_filter(state);
                } else {
                    let changed = self
                        .dialog_command(|bridge| {
                            bridge.request::<_, xedit_session::filter::FilterPresetSaveResponse>(
                                "filter.preset.save",
                                &FilterPresetSaveRequest {
                                    name: last.clone(),
                                    options: Some(state.to_options()),
                                    dry_run: true,
                                },
                            )
                        })
                        .map(|response| response.changed)
                        .unwrap_or(false);
                    if changed {
                        state.question = Some(FilterQuestion { preset: last });
                        self.dialog = Some(Dialog::FilterOptions(Box::new(state)));
                    } else {
                        self.apply_filter(state);
                    }
                }
            }
            FilterOptionsAction::CommitAndSavePreset(name) => {
                self.save_filter_preset(&state, &name);
                self.apply_filter(state);
            }
            FilterOptionsAction::CommitWithoutSaving => self.apply_filter(state),
            FilterOptionsAction::Cancelled => {
                self.save_dialog_placement("frmFilterOptions", rect);
            }
        }
    }

    /// `FilterSavePreset`: the option values as `[Filter <name>]`.
    fn save_filter_preset(&mut self, state: &FilterOptionsState, name: &str) {
        match self.dialog_command(|bridge| {
            bridge.request::<_, xedit_session::filter::FilterPresetSaveResponse>(
                "filter.preset.save",
                &FilterPresetSaveRequest {
                    name: name.to_owned(),
                    options: Some(state.to_options()),
                    dry_run: false,
                },
            )
        }) {
            Ok(response) => self.add_message(&format!(
                "Filter preset \"{}\" {}",
                if name.is_empty() { "(defaults)" } else { name },
                if response.changed { "saved" } else { "unchanged" }
            )),
            Err(error) => self.add_message(&format!("Fatal: {}", error.message)),
        }
    }

    fn refresh_filter_presets(&mut self, state: &mut FilterOptionsState) {
        if let Ok(presets) = self.dialog_command(|bridge| {
            bridge.request::<_, FilterPresetsResponse>("filter.presets", &FilterPresetsRequest {})
        }) {
            state.set_presets(presets.presets);
        }
    }

    /// `FilterApply`: `filter.apply` with the dialog's options and the saved
    /// mod groups, then its lines in the message log.
    fn apply_filter(&mut self, state: FilterOptionsState) {
        let rect = state.rect;
        let request = FilterApplyRequest {
            files: Vec::new(),
            options: Some(state.to_options()),
            preset: None,
            mod_groups: Vec::new(),
            all_mod_groups: false,
            saved_mod_groups: true,
            list_records: false,
            conflict_only: false,
            only_one: false,
            no_game_master: false,
        };
        let params = serde_json::to_value(&request).unwrap_or(serde_json::Value::Null);
        self.start_job(
            "filter.apply",
            params,
            JobSpec {
                caption: "Filtering".to_owned(),
                message: "Applying the filter for cleaning. Please Wait...".to_owned(),
                can_cancel: true,
                show_delay: 200,
                update_delay: 100,
            },
            PendingJob::Filter { rect },
        );
        self.save_dialog_placement("frmFilterOptions", rect);
    }

    // ----- Step 7: the job runner, the modal tools and the two prompts -----

    /// Runs a command on the job runner: the session moves to the worker,
    /// the window keeps repainting, and `TfrmWait` shows the progress and
    /// offers Cancel.
    fn start_job(&mut self, command: &str, params: serde_json::Value, spec: JobSpec, pending: PendingJob) -> bool {
        if self.job.is_running() {
            self.add_message(&format!(
                "{} is still running: wait for it, or press Cancel in its wait form",
                self.job.running_command().unwrap_or("a command")
            ));
            return false;
        }
        let Some(bridge) = self.bridge.take() else {
            self.add_message("No session is loaded");
            return false;
        };
        match self.job.start(bridge, command, params, spec) {
            Ok(()) => {
                self.pending_job = pending;
                true
            }
            Err(error) => {
                // A thread that could not start: the session went with the
                // failed spawn, and the load has to run again.
                self.add_message(&format!("Fatal: {}", error.message));
                false
            }
        }
    }

    /// The result of the finished job: the session back, the progress lines
    /// in the log, and the handler of the command.
    fn poll_job(&mut self) {
        let Some(finished) = self.job.poll() else {
            return;
        };
        let FinishedJob {
            bridge,
            command,
            result,
            messages,
            elapsed,
        } = finished;
        self.bridge = Some(bridge);
        for message in &messages {
            self.add_message(message);
        }
        let pending = std::mem::replace(&mut self.pending_job, PendingJob::Command);
        self.finish_job(command, result, elapsed, pending);
    }

    fn finish_job(
        &mut self,
        command: String,
        result: Result<serde_json::Value, CommandError>,
        elapsed: std::time::Duration,
        pending: PendingJob,
    ) {
        let elapsed = elapsed.as_secs_f32();
        match result {
            Ok(value) => match pending {
                PendingJob::Filter { rect } => {
                    // The counts, the messages and the filtered tree of the
                    // Conflicted and Filter views (`after_command`).
                    self.after_command("filter.apply", &value);
                    self.save_dialog_placement("frmFilterOptions", rect);
                }
                PendingJob::LodgenChecklist => {
                    match serde_json::from_value::<xedit_session::lodgen::LodgenResponse>(value) {
                        Ok(response) => {
                            for message in &response.messages {
                                self.add_message(message);
                            }
                            let mut state = LodgenState::from_response(&response);
                            // `if GetKeyState(VK_SHIFT) < 0 then
                            // btnSplitTreesLOD.Visible := True` (Skyrim and
                            // the Fallouts before 4).
                            state.split_visible = self.shift_held;
                            self.dialog = Some(Dialog::Lodgen(Box::new(state)));
                        }
                        Err(error) => self.add_message(&format!("Fatal: {error}")),
                    }
                }
                PendingJob::LodgenRun => {
                    if let Ok(response) = serde_json::from_value::<xedit_session::lodgen::LodgenResponse>(value) {
                        for message in &response.messages {
                            self.add_message(message);
                        }
                        if let Some(seed) = response.seed {
                            self.add_message(&format!("LODGen RandSeed: {seed}"));
                        }
                    }
                    self.add_message(&format!("LOD generation finished in {elapsed:.1} s"));
                }
                PendingJob::Command => {
                    // The lines xEdit prints and the `NavUpdate` of a
                    // command that changed the tree.
                    self.after_command(&command, &value);
                    self.add_message(&format!("{command} finished in {elapsed:.1} s"));
                }
            },
            Err(error) if error.code == "cancelled" => {
                self.add_message(&format!("{command} cancelled"));
                if let PendingJob::Filter { rect: Some(rect) } = pending {
                    self.save_dialog_placement("frmFilterOptions", Some(rect));
                }
            }
            Err(error) => self.add_message(&format!("Fatal: {}", error.message)),
        }
    }

    /// The keys `mniNavOptionsClick` reads and the values the OK writes.
    fn open_options(&mut self) {
        let values = self.settings_values("Options");
        let settings = self.settings.clone();
        let mut state = options_form::state_from_values(
            &values,
            &settings.fonts,
            &settings.colors,
            settings.theme.clone(),
            xedit_core::interface::globals::i_know_what_im_doing(),
        );
        state.column_width = self.column_width.to_string();
        state.row_height = self.row_height.to_string();
        self.dialog = Some(Dialog::Options(Box::new(state)));
    }

    /// `mniNavOptionsClick` after `ShowModal = mrOK`: the keys of the
    /// `[Options]` section, the colours and the `[UI]` values, then the
    /// globals and the live main form.
    fn apply_options(&mut self, state: OptionsState, ctx: &egui::Context) {
        for (key, value, kind) in state.option_values() {
            self.write_settings("Options", key, serde_json::Value::String(value), kind);
        }
        for (section, key, value) in state.color_values() {
            self.write_settings(section, &key, serde_json::json!(value), "integer");
        }
        for (key, value, kind) in state.ui_values() {
            self.write_settings("UI", &key, serde_json::Value::String(value), kind);
        }
        // The globals the editor reads everywhere.
        state.apply_globals();
        // The live main form: the theme, the fonts and the colours.
        self.settings.theme = state.theme.clone();
        self.settings.fonts.records = state.font_records.clone();
        self.settings.fonts.messages = state.font_messages.clone();
        self.settings.colors = state.colors.clone();
        self.shrink_buttons = state.shrink_buttons;
        self.column_width = str_to_int_def(&state.column_width, self.column_width);
        self.row_height = str_to_int_def(&state.row_height, self.row_height);
        self.theme_applied = false;
        self.settings.theme.apply(ctx);
        self.add_message("Options applied");
    }

    /// `settings.set` over the loaded session, or the pre-load one.
    fn write_settings(&mut self, section: &str, key: &str, value: serde_json::Value, value_type: &str) {
        // `WriteBool` takes a Boolean (the dialog holds "1"/"0").
        let value = match (value_type, value) {
            ("bool", serde_json::Value::String(text)) => {
                serde_json::Value::Bool(text.trim() != "0" && !text.is_empty())
            }
            (_, value) => value,
        };
        let request = SettingsSetRequest {
            section: section.to_owned(),
            key: key.to_owned(),
            value,
            value_type: Some(value_type.to_owned()),
            dry_run: false,
        };
        if let Some(bridge) = self.bridge.as_mut() {
            let _ = bridge.request::<_, SettingsSetResponse>("settings.set", &request);
        } else if let Some(bridge) = self.module_bridge.as_mut() {
            let _ = bridge.request::<_, SettingsSetResponse>("settings.set", &request);
        }
    }

    /// `mniNavGenerateLODClick`: the dry run lists the worldspaces and the
    /// options, then `TfrmLODGen` opens.
    fn open_lodgen(&mut self) {
        let params = serde_json::json!({
            "worldspaces": [],
            "options": {},
            "dry_run": true,
        });
        self.start_job(
            "lodgen.generate",
            params,
            JobSpec {
                caption: "LODGen".to_owned(),
                message: "Loading LOD options. Please Wait...".to_owned(),
                can_cancel: true,
                show_delay: 200,
                update_delay: 100,
            },
            PendingJob::LodgenChecklist,
        );
    }

    /// The OK (and the hidden split) of `TfrmLODGen`.
    fn run_lodgen(&mut self, state: &LodgenState, split_trees: bool) {
        // The settings file of the session, whose `[<APP> LOD Options]`
        // section the command reads and writes back.
        let settings = self.settings.path.as_ref().map(|path| path.display().to_string());
        let params = state.to_params(settings, split_trees);
        let message = if split_trees {
            "Splitting the trees LOD atlases. Please Wait..."
        } else {
            "Generating LOD. Please Wait..."
        };
        self.start_job(
            "lodgen.generate",
            params,
            JobSpec {
                caption: "LODGen".to_owned(),
                message: message.to_owned(),
                can_cancel: true,
                show_delay: 200,
                update_delay: 100,
            },
            PendingJob::LodgenRun,
        );
    }

    /// `bnLegendClick`: the legend of the conflict colours.
    fn open_legend(&mut self) {
        let placement = match self.bridge.as_mut() {
            Some(bridge) => read_placement(bridge, legend_form::FORM_SECTION),
            None => None,
        };
        self.dialog = Some(Dialog::Legend(Box::new(LegendState::new(placement))));
    }

    /// `TfrmMain.EditWarn`: the warning before the first edit.
    fn open_edit_warning(&mut self) {
        let count = self
            .settings_values("Usage")
            .into_iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("EditWarnCount"))
            .map(|(_, value)| str_to_int_def(&value, 0))
            .unwrap_or(0);
        self.dialog = Some(Dialog::EditWarning(Box::new(edit_warning_form::edit_warning_state(
            count,
        ))));
    }
}

/// The caption of the module list of `mniNavUpdateCRCModGroupsClick`, which
/// follows the answers of its two questions.
fn update_crc_caption(answers: &[NavAnswer]) -> String {
    let add = answers
        .first()
        .is_some_and(|answer| matches!(answer, NavAnswer::Yes(true)));
    let update = answers
        .get(1)
        .is_some_and(|answer| matches!(answer, NavAnswer::Yes(true)));
    match (add, update) {
        (true, true) => {
            "Please select the modules for which you want to add or update the current CRC to/in ModGroup Items..."
                .to_owned()
        }
        (true, false) => "Please select the modules for which you want to add the current CRC to ModGroups".to_owned(),
        _ => "Please select the modules for which you want to update the current CRC in ModGroups...".to_owned(),
    }
}

/// The value of an element of a record, by the names of its path: the
/// elements of `records.get` are a tree, and the shell looks for the
/// `MODL\MODL` of an idle in it.
fn find_element_value(value: &serde_json::Value, first: &str, second: &str) -> Option<String> {
    let elements = value.get("elements")?.as_array()?;
    for element in elements {
        if element.get("name").and_then(|name| name.as_str()) != Some(first) {
            continue;
        }
        let children = element.get("children")?.as_array()?;
        for child in children {
            if child.get("name").and_then(|name| name.as_str()) == Some(second) {
                return child.get("value").and_then(|value| value.as_str()).map(str::to_owned);
            }
        }
    }
    None
}

/// Reads `[<section>] Left/Top/Width/Height` through `settings.get`.
fn read_placement(bridge: &mut CommandBridge, section: &str) -> Option<DialogPlacement> {
    let response: SettingsGetResponse = bridge
        .request(
            "settings.get",
            &SettingsGetRequest {
                section: Some(section.to_owned()),
                key: None,
                value_type: None,
                default: None,
            },
        )
        .ok()?;
    let values: Vec<(String, String)> = response
        .values
        .into_iter()
        .map(|entry| (entry.key, entry.raw))
        .collect();
    DialogPlacement::from_values(&values)
}

/// Writes the placement of a dialog through `settings.set`.
fn write_placement(bridge: &mut CommandBridge, section: &str, rect: egui::Rect) {
    for (key, value) in DialogPlacement::from_rect(rect).values() {
        let _ = bridge.request::<_, SettingsSetResponse>(
            "settings.set",
            &SettingsSetRequest {
                section: section.to_owned(),
                key: key.to_owned(),
                value: serde_json::json!(value),
                value_type: Some("integer".to_owned()),
                dry_run: false,
            },
        );
    }
}

/// Calls `visit` for every item id of a menu tree, submenus included.
fn collect_menu_ids(items: &'static [MenuItem], visit: &mut dyn FnMut(&'static str)) {
    for item in items {
        visit(item.id);
        collect_menu_ids(item.children, visit);
    }
}

/// Draws a popup menu from the `xeMainForm.dfm` model. A
/// `enabled` predicate applies the item visibility rules of the popup
/// (`pmuViewPopup`): an item it refuses is drawn disabled. The `runtime`
/// menus hold the items upstream builds when the popup opens (the language
/// list of `pmuMainPopup`).
fn draw_menu(
    ui: &mut egui::Ui,
    tree: &'static MenuTree,
    enabled: &dyn Fn(&'static str) -> bool,
    runtime: &[menu_model::RuntimeMenu],
) -> Option<ChosenMenu> {
    let mut chosen = None;
    draw_items(ui, tree.items, enabled, runtime, &mut chosen);
    chosen
}

/// `mniMainPluggyLink.Visible` (`xeMainForm.pas:15384`): the game is
/// Oblivion or the `xEdit\xEditLink.ini` file of the data path is there.
fn pluggy_link_visible() -> bool {
    if globals::game_mode() == GameMode::gmTES4 {
        return true;
    }
    std::path::Path::new(&globals::data_path())
        .join("xEdit")
        .join("xEditLink.ini")
        .exists()
}

/// `wbIsOblivion or wbIsSkyrim`: the games whose `WMUserLoaderDone` shows
/// the three spreadsheets.
fn spreadsheets_for_game(game: GameMode) -> bool {
    matches!(
        game,
        GameMode::gmTES4
            | GameMode::gmTES4R
            | GameMode::gmTES5
            | GameMode::gmEnderal
            | GameMode::gmTES5VR
            | GameMode::gmSSE
            | GameMode::gmEnderalSE
    )
}

/// Draws `pmuNav` and `pmuNavHeaderPopup`: every item resolves through
/// [`crate::nav_tree::nav_actions`]. An item the selection does not apply to
/// is disabled; an item whose dialog belongs to a later step stays enabled
/// and its tooltip names that dialog.
fn draw_nav_menu(
    ui: &mut egui::Ui,
    tree: &'static MenuTree,
    selection: &Selection,
    hidden_checked: bool,
    edit_allowed: bool,
    add_list: &[String],
    has_mod_groups: bool,
) -> Vec<NavAction> {
    let mut actions = Vec::new();
    draw_nav_items(
        ui,
        tree.items,
        selection,
        hidden_checked,
        edit_allowed,
        add_list,
        has_mod_groups,
        &mut actions,
    );
    actions
}

#[allow(clippy::too_many_arguments)]
fn draw_nav_items(
    ui: &mut egui::Ui,
    items: &'static [MenuItem],
    selection: &Selection,
    hidden_checked: bool,
    edit_allowed: bool,
    add_list: &[String],
    has_mod_groups: bool,
    actions: &mut Vec<NavAction>,
) {
    // `TfrmMain.pmuNavPopup` (`xeMainForm.pas:15410-15680`): the popup
    // shows the items that apply to the focused element and hides the
    // others.
    let context = menu_model::NavContext {
        selection,
        edit_allowed,
        add_list_len: add_list.len(),
        light_supported: globals::is_light_supported(),
        game: globals::game_mode(),
        has_mod_groups,
    };
    for item in items {
        if !item.visible {
            continue;
        }
        // `xeMainForm.pas:15478`: the manual cleaning items come in a
        // current and an obsolete variant; only one of each pair is shown.
        // `mniNavCheckForErrors.Visible` is `wbEditAllowed` and an
        // assigned element.
        let check_for_errors = edit_allowed && selection.focused.is_some();
        if let Some(visible) = menu_model::manual_cleaning_visible(
            item.id,
            globals::manual_cleaning_allow(),
            globals::manual_cleaning_hide(),
            check_for_errors,
        ) && !visible
        {
            continue;
        }
        if !menu_model::nav_item_visible(item.id, &context) {
            continue;
        }
        match item.kind {
            MenuKind::Separator => {
                ui.separator();
            }
            MenuKind::Submenu => {
                ui.menu_button(item.caption, |ui| {
                    draw_nav_items(
                        ui,
                        item.children,
                        selection,
                        hidden_checked,
                        edit_allowed,
                        add_list,
                        has_mod_groups,
                        actions,
                    );
                });
            }
            MenuKind::RuntimeSubmenu => {
                // The navigation tree has no menu whose items are built at
                // run time (`mniNavAdd` is an action with its own list).
                ui.add_enabled(false, egui::Button::new(item.caption))
                    .on_disabled_hover_text("The items of this menu are built at run time");
            }
            MenuKind::Action => {
                if item.id == "mniNavAdd" {
                    // `mniNavAddClick`: one item per `Container.GetAddList`
                    // entry; the caption is what `elements.add` receives.
                    if add_list.is_empty() {
                        ui.add_enabled(false, egui::Button::new(item.caption))
                            .on_disabled_hover_text("Nothing can be added to the focused element");
                    } else {
                        ui.menu_button(item.caption, |ui| {
                            for caption in add_list {
                                if ui.button(caption).clicked() {
                                    actions.push(crate::nav_tree::add_item_action(caption, selection));
                                    ui.close();
                                }
                            }
                        });
                    }
                    continue;
                }
                let item_actions = crate::nav_tree::nav_actions(item, selection, edit_allowed);
                let applicable = !matches!(item_actions.first(), Some(NavAction::None));
                let enabled = item.enabled && applicable;
                let checked = if item.id == "mniNavHidden" {
                    hidden_checked
                } else {
                    item.checked
                };
                // `xeMainForm.pas:15600`: the caption of the item holds the
                // number of the records it compares.
                let caption = if item.id == "mniNavCompareSelected" {
                    format!("Compare Selected ({})", context.compare_count())
                } else {
                    item.caption.to_owned()
                };
                let mut button = egui::Button::new(caption).selected(checked);
                if let Some(shortcut) = item.shortcut {
                    button = button.shortcut_text(shortcut);
                }
                let response = ui.add_enabled(enabled, button);
                let response = if !applicable {
                    response.on_disabled_hover_text("Not applicable to the current selection")
                } else {
                    response
                };
                if enabled && response.clicked() {
                    actions.extend(item_actions);
                    ui.close();
                }
            }
        }
    }
}

fn draw_items(
    ui: &mut egui::Ui,
    items: &'static [MenuItem],
    enabled: &dyn Fn(&'static str) -> bool,
    runtime: &[menu_model::RuntimeMenu],
    chosen: &mut Option<ChosenMenu>,
) {
    for item in items {
        if !item.visible {
            continue;
        }
        // `xeMainForm.pas:15384`: the pluggy link items are only shown for
        // Oblivion or when the `xEdit\xEditLink.ini` file of the data path
        // is there.
        if item.id.starts_with("mniMainPluggyLink") && !pluggy_link_visible() {
            continue;
        }
        match item.kind {
            MenuKind::Separator => {
                ui.separator();
            }
            MenuKind::Submenu => {
                ui.menu_button(item.caption, |ui| {
                    draw_items(ui, item.children, enabled, runtime, chosen);
                });
            }
            MenuKind::RuntimeSubmenu => {
                // Upstream fills the item when the popup opens
                // (`pmuMainPopup`): the language list of the Localization
                // submenu, one radio item per language of the session.
                let Some(menu) = runtime.iter().find(|menu| menu.id == item.id) else {
                    continue;
                };
                if menu.items.is_empty() {
                    ui.add_enabled(false, egui::Button::new(item.caption))
                        .on_disabled_hover_text("No language of the session is available");
                    continue;
                }
                ui.menu_button(item.caption, |ui| {
                    for entry in &menu.items {
                        let response = ui.add(egui::RadioButton::new(entry.checked, &entry.caption));
                        if response.clicked() {
                            *chosen = Some((
                                item.id,
                                crate::widgets::MenuCommand::SessionWithParam {
                                    command: entry.command,
                                    param: entry.param.clone(),
                                },
                            ));
                            ui.close();
                        }
                    }
                });
            }
            MenuKind::Action => {
                let applicable = enabled(item.id);
                let enabled_item = item.enabled && applicable;
                let mut button = egui::Button::new(item.caption).selected(item.checked);
                if let Some(shortcut) = item.shortcut {
                    button = button.shortcut_text(shortcut);
                }
                let response = ui.add_enabled(enabled_item, button);
                if !applicable {
                    response.on_disabled_hover_text("Nothing to act on in the focused view cell");
                } else if response.clicked() {
                    // The form runs every item after the menu closes, so a
                    // command that rebuilds a panel is safe.
                    *chosen = Some((item.id, crate::widgets::menu_command(item)));
                    ui.close();
                }
            }
        }
    }
}

impl TfrmMain {
    /// Runs one typed command through the bridge; a failure goes to the
    /// message log and answers `None`.
    fn call<Q, R>(&mut self, name: &str, request: &Q) -> Option<R>
    where
        Q: serde::Serialize,
        R: DeserializeOwned,
    {
        let Some(bridge) = self.bridge.as_mut() else {
            self.messages.push("No session".to_owned());
            return None;
        };
        match bridge.request::<Q, R>(name, request) {
            Ok(response) => Some(response),
            Err(error) => {
                self.messages.push(format!("{name}: {}", error.message));
                None
            }
        }
    }

    /// Opens one of the step-6 dialogs (a menu command, `--dialog`, or the
    /// startup flow).
    pub fn open_dialog(&mut self, kind: DialogKind) {
        if self.dialogs.is_open(kind) {
            return;
        }
        match kind {
            DialogKind::LogAnalyzer => {
                let mut state = LogAnalyzerState::new(LogType::Papyrus);
                state.set_default_file(&data_path(), &my_games_the_game_path());
                self.dialogs.log_analyzer = Some(state);
            }
            DialogKind::Script => {
                let Some(bridge) = self.bridge.as_mut() else {
                    return;
                };
                let last_used = setting_get_string(bridge, "View", "LastUsedScript", "");
                let subdirectories = setting_get_bool(bridge, "View", "IncludeScriptsFromSubDir", false);
                self.dialogs.script = Some(ScriptFormState::new(None, last_used, subdirectories));
            }
            DialogKind::ViewElements => {
                // The pages of `TfrmViewElements` are the elements of the
                // focused view row, one page per record; the dialog alone
                // (`--dialog view-elements`) has no row to show, so it says
                // so.
                let mut state = ViewElementsState::default();
                if let Some(bridge) = self.bridge.as_mut() {
                    state.compare_cmdline =
                        setting_get_string(bridge, "External", "CompareCommandLine", "bcompare.exe %1 %2");
                    state.placement.left = setting_get_integer(bridge, "frmViewElements", "Left", 0) as i32;
                    state.placement.top = setting_get_integer(bridge, "frmViewElements", "Top", 0) as i32;
                    state.placement.width =
                        setting_get_integer(bridge, "frmViewElements", "Width", 565).max(200) as i32;
                    state.placement.height =
                        setting_get_integer(bridge, "frmViewElements", "Height", 325).max(150) as i32;
                    state.placement_loaded = true;
                }
                let text = "No element is selected.";
                state.tabs.push(ElementTab {
                    caption: "no element".to_owned(),
                    file: String::new(),
                    text: text.to_owned(),
                    edited: text.to_owned(),
                    editable: false,
                    modified: false,
                    form_id: String::new(),
                    path: String::new(),
                });
                self.dialogs.view_elements = Some(state);
            }
            DialogKind::RichEdit => {
                let Some(bridge) = self.bridge.as_mut() else {
                    return;
                };
                let toc = setting_get_bool(bridge, "frmRichEdit", "TOC", false);
                let toc_width = setting_get_integer(bridge, "frmRichEdit", "TOCWidth", 185) as i32;
                let zoom = setting_get_integer(bridge, "frmRichEdit", "Zoom", 100) as i32;
                let document = crate::dialogs::viewers::parse_rtf(crate::dialogs::rich_edit_form::WHATS_NEW_RTF);
                self.dialogs.rich_edit = Some(RichEditState::new("What's New?", document, toc, toc_width, zoom));
            }
            DialogKind::Localization => {
                self.dialogs.localization = Some(LocalizationState::new());
            }
            DialogKind::DeveloperMessage => {
                let Some(bridge) = self.bridge.as_mut() else {
                    return;
                };
                let patron = setting_get_bool(bridge, "Options", "Patron", false);
                self.dialogs.developer_message = Some(DeveloperMessageState::new(patron));
            }
            DialogKind::Tip => {
                let show_tip = self
                    .bridge
                    .as_mut()
                    .map(|bridge| setting_get_bool(bridge, "Options", "ShowTip", true))
                    .unwrap_or(true);
                let app_name = self.game.map(xedit_session::game_paths::app_name).unwrap_or_default();
                let folder = self.program.parent().map(PathBuf::from).unwrap_or_default();
                self.dialogs.tip = Some(TipState::new(&folder, app_name, show_tip));
            }
            DialogKind::LocalizePlugin => {
                let tables = self
                    .call::<_, xedit_session::localization::LocalizationLanguagesResponse>(
                        "localization.languages",
                        &xedit_session::NoParams {},
                    )
                    .map(|response| response.tables)
                    .unwrap_or_default();
                self.dialogs.localize_plugin = Some(LocalizePluginState::new(tables));
            }
        }
    }

    /// `DoInit`'s developer message and `ShowTip` after the load.
    fn startup_dialogs(&mut self) {
        if self.pending_dialog.is_some() {
            return;
        }
        const DEVELOPER_MESSAGE_VERSION: i64 = 1;
        let Some(bridge) = self.bridge.as_mut() else {
            return;
        };
        let patron = setting_get_bool(bridge, "Options", "Patron", false);
        let version = setting_get_integer(bridge, "DeveloperMessage", "Version", 0);
        let last_shown = setting_get_integer(bridge, "DeveloperMessage", "LastShownOn", 0);
        let show_tip = setting_get_bool(bridge, "Options", "ShowTip", true);
        let today = xedit_session::script_rtl::datetime::today() as i64;
        if version < DEVELOPER_MESSAGE_VERSION || (!patron && last_shown + 14 <= today) {
            self.open_dialog(DialogKind::DeveloperMessage);
        }
        if show_tip {
            self.open_dialog(DialogKind::Tip);
        }
    }

    /// Draws every open dialog and runs its action.
    fn show_dialogs(&mut self, ctx: &egui::Context) {
        // `--dialog <name>`: a step-6 viewer or editor opens itself here.
        // The step-5 selection dialogs keep their own `--dialog` names and
        // are opened by `open_pending_dialog` once the session is loaded.
        // The dialog waits for the load: upstream's own dialogs open when
        // their form is shown, and a dialog opened over a load that is
        // still running has nothing to show. The name is therefore kept
        // until the load has finished, or `finish_load` would find none
        // and open the developer message over the requested dialog.
        if !matches!(self.startup, Startup::Loading)
            && let Some(kind) = self.pending_dialog.as_deref().and_then(DialogKind::from_name)
        {
            self.pending_dialog = None;
            self.open_dialog(kind);
        }
        if self.dialogs.any_open() {
            // A dialog repaints while it is open, as a modal form does: the
            // screen check captures a painted frame, not an empty backbuffer.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if let Some(mut state) = self.dialogs.log_analyzer.take() {
            let action = crate::dialogs::log_analyzer_form::show(ctx, &mut state);
            if self.handle_log_analyzer(&mut state, action) {
                self.dialogs.log_analyzer = Some(state);
            }
        }
        if let Some(mut state) = self.dialogs.script.take() {
            let action = crate::dialogs::script_form::show(ctx, &mut state);
            if self.handle_script(&mut state, action) {
                self.dialogs.script = Some(state);
            }
        }
        if let Some(mut state) = self.dialogs.view_elements.take() {
            let action = crate::dialogs::view_elements_form::show(ctx, &mut state);
            if self.handle_view_elements(&mut state, action) {
                self.dialogs.view_elements = Some(state);
            }
        }
        if let Some(mut state) = self.dialogs.localization.take() {
            let action = crate::dialogs::localization_form::show(ctx, &mut state);
            if self.handle_localization(&mut state, action) {
                self.dialogs.localization = Some(state);
            }
        }
        if let Some(mut state) = self.dialogs.rich_edit.take() {
            let action = crate::dialogs::rich_edit_form::show(ctx, &mut state);
            if self.handle_rich_edit(action) {
                self.dialogs.rich_edit = Some(state);
            }
        }
        if let Some(mut state) = self.dialogs.developer_message.take() {
            let action = crate::dialogs::developer_message_form::show(ctx, &mut state);
            if self.handle_developer_message(ctx, &state, action) {
                self.dialogs.developer_message = Some(state);
            }
        }
        if let Some(mut state) = self.dialogs.tip.take() {
            let action = crate::dialogs::tip_form::show(ctx, &mut state);
            if self.handle_tip(action) {
                self.dialogs.tip = Some(state);
            }
        }
        if let Some(mut state) = self.dialogs.localize_plugin.take() {
            let action = crate::dialogs::localize_plugin_form::show(ctx, &mut state);
            if self.handle_localize_plugin(action) {
                self.dialogs.localize_plugin = Some(state);
            }
        }
    }

    fn handle_log_analyzer(&mut self, state: &mut LogAnalyzerState, action: Option<LogAnalyzerAction>) -> bool {
        match action {
            None => true,
            Some(LogAnalyzerAction::Closed) => false,
            Some(LogAnalyzerAction::JumpTo { form_id, file }) => {
                // `TfrmLogAnalyzerForm`'s jump: the record goes into the
                // navigation tree and the view.
                self.reveal_in_navigation_tree(&form_id, Some(file.as_str()));
                self.set_pane_record(Some(RecordSelection::new(form_id, Some(file))));
                true
            }
            Some(LogAnalyzerAction::Analyze { file, max_size_mb }) => {
                let request = LogsAnalyzeRequest {
                    log: state.log,
                    file: Some(file),
                    max_size_mb: Some(max_size_mb),
                };
                if let Some(response) = self.call::<_, LogsAnalyzeResponse>("logs.analyze", &request) {
                    state.apply_analysis(response);
                }
                true
            }
        }
    }

    fn handle_script(&mut self, state: &mut ScriptFormState, action: Option<ScriptFormAction>) -> bool {
        match action {
            None => true,
            Some(ScriptFormAction::Cancelled) => false,
            Some(ScriptFormAction::List { folder, subdirectories }) => {
                let request = ScriptListRequest {
                    scripts: folder,
                    subdirectories,
                };
                match self.call::<_, ScriptListResponse>("script.list", &request) {
                    Some(response) => match state.apply_list(response) {
                        Some(next) => self.handle_script(state, Some(next)),
                        None => true,
                    },
                    None => {
                        state.error = Some("Could not read the scripts folder".to_owned());
                        true
                    }
                }
            }
            Some(ScriptFormAction::Read { file }) => self.script_read(state, file),
            Some(ScriptFormAction::Save { file, text }) => {
                self.script_save(state, file, text);
                true
            }
            Some(ScriptFormAction::SaveThenRead { file, text, read }) => {
                if self.script_save(state, file, text) {
                    self.script_read(state, read)
                } else {
                    true
                }
            }
            Some(ScriptFormAction::SaveThenOk { file, text, name }) => {
                if self.script_save(state, file, text.clone()) {
                    self.script_run(name, text)
                } else {
                    true
                }
            }
            Some(ScriptFormAction::Ok { name, text }) => self.script_run(name, text),
        }
    }

    fn script_read(&mut self, state: &mut ScriptFormState, file: String) -> bool {
        let request = ScriptReadRequest { file, scripts: None };
        match self.call::<_, ScriptReadResponse>("script.read", &request) {
            Some(response) => state.apply_read(response),
            None => {
                state.error = Some(format!("Could not read {}", request.file));
            }
        }
        true
    }

    /// Saves a script; whether it was written.
    fn script_save(&mut self, state: &mut ScriptFormState, file: String, text: String) -> bool {
        let request = ScriptSaveRequest {
            file,
            text,
            scripts: None,
            dry_run: false,
        };
        match self.call::<_, ScriptSaveResponse>("script.save", &request) {
            Some(response) => {
                state.apply_save(response);
                true
            }
            None => {
                state.error = Some(format!("Could not save {}", request.file));
                false
            }
        }
    }

    /// `FormClose` with `mrOk`: runs the editor text with `script.run` and
    /// writes the `[View] LastUsedScript` key the form writes.
    fn script_run(&mut self, name: String, text: String) -> bool {
        let dry_run = !self.edit_allowed;
        let request = ScriptRunRequest {
            file: format!("{name}.pas"),
            scripts: None,
            source: Some(text),
            save: !dry_run,
            dry_run,
            build_refs: false,
            answers: Vec::new(),
        };
        if let Some(response) = self.call::<_, ScriptRunResponse>("script.run", &request) {
            for message in response.messages {
                self.messages.push(message);
            }
            self.add_message(&format!("Applied script \"{name}\""));
        }
        if let Some(bridge) = self.bridge.as_mut() {
            let _ = setting_set(
                bridge,
                "View",
                "LastUsedScript",
                serde_json::Value::String(name),
                "string",
            );
        }
        false
    }

    fn handle_view_elements(&mut self, _state: &mut ViewElementsState, action: Option<ViewElementsAction>) -> bool {
        match action {
            None => true,
            Some(ViewElementsAction::Closed) => false,
            Some(ViewElementsAction::Save { updates }) => {
                for update in updates {
                    let request = ElementsSetRequest {
                        form_id: update.form_id,
                        path: update.path,
                        file: update.file,
                        value: serde_json::Value::String(update.text),
                        dry_run: false,
                    };
                    if let Some(response) = self.call::<_, ElementsSetResponse>("elements.set", &request) {
                        self.messages.push(format!(
                            "{}: {}",
                            response.path,
                            if response.changed { "changed" } else { "unchanged" }
                        ));
                    }
                }
                false
            }
        }
    }

    fn handle_localization(&mut self, state: &mut LocalizationState, action: Option<LocalizationAction>) -> bool {
        match action {
            None => true,
            Some(LocalizationAction::Closed) => false,
            Some(LocalizationAction::Files) => {
                let request = LocalizationFilesRequest { file: None, load: true };
                match self.call::<_, LocalizationFilesResponse>("localization.files", &request) {
                    Some(response) => state.apply_files(response.tables),
                    None => state.error = Some("Could not list the string tables".to_owned()),
                }
                true
            }
            Some(LocalizationAction::Strings { table }) => {
                let request = LocalizationStringsRequest {
                    table: table.clone(),
                    offset: 0,
                    limit: Some(100_000),
                };
                match self.call::<_, LocalizationStringsResponse>("localization.strings", &request) {
                    Some(response) => state.apply_strings(&table, response),
                    None => state.error = Some(format!("Could not read the strings of {table}")),
                }
                true
            }
            Some(LocalizationAction::Set { table, id, text }) => {
                let request = LocalizationSetRequest {
                    table: Some(table),
                    id: Some(id),
                    form_id: None,
                    path: None,
                    file: None,
                    text,
                    editor_text: true,
                    dry_run: false,
                };
                if let Some(response) = self.call::<_, LocalizationSetResponse>("localization.set", &request) {
                    state.apply_save(&response.table, &response.id, response.new);
                }
                true
            }
            Some(LocalizationAction::Export { table, output }) => {
                let request = LocalizationExportRequest {
                    table,
                    output,
                    dry_run: false,
                };
                if let Some(response) = self
                    .call::<_, xedit_session::localization::LocalizationExportResponse>("localization.export", &request)
                {
                    self.messages.push(format!(
                        "Exported {} strings of {} to {}",
                        response.strings, response.table, response.output
                    ));
                }
                true
            }
        }
    }

    fn handle_rich_edit(&mut self, action: Option<RichEditAction>) -> bool {
        match action {
            None => true,
            Some(RichEditAction::Closed) => false,
            Some(RichEditAction::Ok { dont_show_again }) => {
                if dont_show_again && let Some(bridge) = self.bridge.as_mut() {
                    // `wbWhatsNewVersion`.
                    let _ = setting_set(bridge, "WhatsNew", "Version", serde_json::Value::from(1), "integer");
                }
                false
            }
            Some(RichEditAction::Settings { toc, toc_width, zoom }) => {
                if let Some(bridge) = self.bridge.as_mut() {
                    let _ = setting_set(bridge, "frmRichEdit", "TOC", serde_json::Value::Bool(toc), "bool");
                    let _ = setting_set(
                        bridge,
                        "frmRichEdit",
                        "TOCWidth",
                        serde_json::Value::from(toc_width),
                        "integer",
                    );
                    let _ = setting_set(bridge, "frmRichEdit", "Zoom", serde_json::Value::from(zoom), "integer");
                }
                true
            }
        }
    }

    fn handle_developer_message(
        &mut self,
        ctx: &egui::Context,
        _state: &DeveloperMessageState,
        action: Option<DeveloperMessageAction>,
    ) -> bool {
        match action {
            None => true,
            Some(DeveloperMessageAction::Timer) => true,
            Some(DeveloperMessageAction::Closed {
                choice,
                dont_show_again,
                patron_cleared,
                open_url,
            }) => {
                if open_url {
                    ctx.open_url(egui::OpenUrl::new_tab(choice.url()));
                }
                let today = xedit_session::script_rtl::datetime::today() as i64;
                if let Some(bridge) = self.bridge.as_mut() {
                    let _ = setting_set(
                        bridge,
                        "DeveloperMessage",
                        "LastShownOn",
                        serde_json::Value::from(today),
                        "integer",
                    );
                    let _ = setting_set(
                        bridge,
                        "DeveloperMessage",
                        "Version",
                        serde_json::Value::from(if dont_show_again { 1 } else { 0 }),
                        "integer",
                    );
                    if patron_cleared {
                        let _ = setting_set(bridge, "Options", "Patron", serde_json::Value::Bool(false), "bool");
                    }
                }
                false
            }
        }
    }

    fn handle_tip(&mut self, action: Option<TipAction>) -> bool {
        match action {
            None => true,
            Some(TipAction::Closed { show_tip }) => {
                if let Some(bridge) = self.bridge.as_mut() {
                    let _ = setting_set(bridge, "Options", "ShowTip", serde_json::Value::Bool(show_tip), "bool");
                }
                false
            }
        }
    }

    fn handle_localize_plugin(&mut self, action: Option<LocalizePluginAction>) -> bool {
        match action {
            None => true,
            Some(LocalizePluginAction::Cancelled) => false,
            Some(LocalizePluginAction::Localize {
                translate_from,
                translate_to,
            }) => {
                let request = LocalizationSwitchRequest {
                    file: None,
                    translate_from,
                    translate_to,
                    dry_run: false,
                };
                if let Some(response) = self.call::<_, LocalizationSwitchResponse>("localization.localize", &request) {
                    self.messages.push(format!(
                        "{}: {} localizable strings, {} translated",
                        response.file, response.localizable, response.translated
                    ));
                }
                false
            }
        }
    }
}

/// The window's loop. Repaint policy: egui calls `ui` only when a repaint
/// is requested, and nothing here requests one per frame. A
/// `ViewportCommand` requests a repaint of its viewport
/// (`Context::send_viewport_cmd_to`), so each is sent once, on change — the
/// title and icon were sent per frame once and idled a full CPU core on the
/// screen-check desktop. `request_repaint_after` is scheduled only while
/// something animates (the loading gauge) or a job is waited on (the load,
/// `--screenshot`).
///
/// A hidden window runs no `ui` pass in this eframe: the integration runs
/// `logic` only, on each repaint request. The screen-check launches every
/// window hidden, so the load result (which would otherwise be polled for
/// the first time in `ui`) and the `--screenshot` state machine are polled
/// here too; a `RequestPaintWhileHidden` from the capture asks for the
/// painted frame it waits on.
impl eframe::App for TfrmMain {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_load();
        self.poll_screenshot(ctx, false);
        if matches!(self.startup, Startup::Loading) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

impl TfrmMain {
    /// One frame of the shell: `App::ui`, without the eframe frame.
    fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // `XEDIT_GUI_SCALE`: the screen-check harness renders the logical
        // sizes of the DFM on a desktop whose system scale would otherwise
        // make a dialog larger than the desktop.
        if let Ok(scale) = std::env::var("XEDIT_GUI_SCALE").unwrap_or_default().parse::<f32>()
            && scale > 0.1
        {
            ctx.set_pixels_per_point(scale);
        }
        self.frame_count += 1;
        if self.frame_count % 60 == 1 {
            let pp = ctx.pixels_per_point();
            let native = ctx.native_pixels_per_point();
            let content = ctx.input(|input| input.content_rect());
            let viewport = ctx.input(|input| input.viewport_rect());
            let monitor = ctx.input(|input| input.viewport().monitor_size);
            crate::log_screen(
                "geometry",
                &[format!(
                    "pp={pp} native={native:?} content={content:?} viewport={viewport:?} monitor={monitor:?}"
                )],
            );
        }
        self.drain_progress();
        self.poll_load();
        self.poll_screenshot(&ctx, true);
        // Step 7: the jobs off the UI thread and the questions of the two
        // core callbacks.
        self.shift_held = ctx.input(|input| input.modifiers.shift);
        self.poll_job();
        self.poll_prompts();
        // `XEDIT_GUI_AUTOCANCEL_MS`: the screen check's Cancel press.
        if let Some(at) = self.auto_cancel
            && Instant::now() >= at
        {
            self.auto_cancel = None;
            if let Some(wait) = &self.load_wait {
                wait.cancel();
            }
            if self.job.is_running() {
                self.job.cancel();
            }
        }
        self.apply_theme(&ctx);

        // The native window: the title and the icon of the chosen game.
        // Both are `ViewportCommand`s, and sending one asks egui for a
        // repaint: they are sent once, when the game is chosen, not every
        // frame (a per-frame send idles a CPU core, seen on the
        // screen-check desktop before this).
        if !self.window_titled
            && let Some(game) = self.game
        {
            self.window_titled = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(application_title(game)));
            if let Some(icon) = crate::icon::icon_for_game(game) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(icon))));
            }
        }
        if !self.window_placed && !self.dialog_only {
            self.window_placed = true;
            // The ini holds the physical pixels `TForm.Left/Width` are, as
            // the Delphi build writes them; egui's viewport commands take
            // logical points. The size is cut to the monitor, because a
            // window larger than it (a 4K ini on a small screen, or the
            // private desktop of the screen check) loses its surface.
            let scale = ctx.pixels_per_point().max(0.5);
            let native = ctx.native_pixels_per_point().unwrap_or(scale);
            let monitor = ctx
                .input(|input| input.viewport().monitor_size)
                .unwrap_or(egui::vec2(1600.0, 900.0));
            let monitor = monitor * native;
            // 7600 physical pixels stays under the 8192 maximum texture
            // side of the adapters this machine has.
            let width = (self.settings.window.width as f32)
                .min(monitor.x * 0.95)
                .clamp(640.0 * scale, 7600.0);
            let height = (self.settings.window.height as f32)
                .min(monitor.y * 0.9)
                .clamp(400.0 * scale, 7600.0);
            // `XEDIT_GUI_WINDOW=<w>x<h>`: the screen-check harness names
            // the native logical size of the window, because a private
            // desktop clamps a larger one without telling winit.
            let mut window_size = egui::vec2(width / scale, height / scale);
            if let Ok(text) = std::env::var("XEDIT_GUI_WINDOW")
                && let Some((w, h)) = text.split_once('x')
                && let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>())
            {
                window_size = egui::vec2(w, h);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(window_size));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                (self.settings.window.left as f32 / scale).max(0.0),
                (self.settings.window.top as f32 / scale).max(0.0),
            )));
            if self.settings.window.state == WindowState::Maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
        }
        if matches!(self.startup, Startup::Loading) || self.job.is_running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // `TfrmWait` of the startup load and of a job; Cancel is
        // upstream's `bnCancelClick`. `fwDoUpdate` shows the form after
        // `fwShowDelay` and pumps its messages every `fwUpdateDelay`.
        if let Some(wait) = &self.load_wait {
            if wait.update_due(&mut self.last_wait_update) {
                ctx.request_repaint();
            }
            if let Some(WaitFormAction::Cancel) = wait_form::show(&ctx, wait) {
                // Upstream `btnCancelClick` asks first; the flag is set
                // only after the question is answered with yes.
                wait.request_cancel();
            }
            if let Some(yes) = wait_form::show_cancel_question(&ctx, wait) {
                wait.answer_cancel(yes);
            }
        }
        if let Some(wait) = self.job.wait_form().cloned() {
            if let Some(WaitFormAction::Cancel) = wait_form::show(&ctx, &wait) {
                self.job.request_cancel();
            }
            if let Some(yes) = wait_form::show_cancel_question(&ctx, &wait) {
                self.job.answer_cancel(yes);
            }
        }
        // `FormClose`: the placement and the splitter are written once, when
        // the window is asked to close.
        if ctx.input(|input| input.viewport().close_requested()) && !self.settings_saved {
            self.settings_saved = true;
            self.save_settings(&ctx);
        }

        if self.dialog_only {
            // `--dialog`: the dialog alone, so the screen check sees it
            // without the shell behind it. One point is one pixel, so the
            // dialogs' point sizes fit the window whole. The empty central
            // panel gives the frame its surface; the dialog's Areas draw
            // over it.
            ctx.set_zoom_factor(1.0);
            egui::CentralPanel::default_margins().show(ui, |_ui| {});
            self.show_dialogs(&ctx);
            return;
        }
        // The screen is copied out first: the arms below take `&mut self`.
        let screen = match &self.startup {
            Startup::GameSelect(_) => Screen::GameSelect,
            Startup::ModuleSelect(_) => Screen::ModuleSelect,
            Startup::Loading => Screen::Loading,
            Startup::Failed(_) => Screen::Failed,
            Startup::Loaded => Screen::Loaded,
        };
        let screen_name = match screen {
            Screen::GameSelect => "GameSelect",
            Screen::ModuleSelect => "ModuleSelect",
            Screen::Loading => "Loading",
            Screen::Failed => "Failed",
            Screen::Loaded => "Loaded",
        };
        if self.logged_screen != Some(screen_name) {
            self.logged_screen = Some(screen_name);
            crate::log_screen(screen_name, &self.messages.lines);
        }
        match screen {
            Screen::GameSelect => {
                self.status_bar(ui);
                self.game_select_screen(&ctx);
            }
            Screen::ModuleSelect => {
                self.status_bar(ui);
                self.module_select_screen(&ctx);
            }
            Screen::Loading => {
                self.status_bar(ui);
                egui::CentralPanel::default_margins().show(ui, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(format!(
                            "Loading plugins... {:.0} s",
                            self.load_started.elapsed().as_secs_f32()
                        ));
                    });
                });
            }
            Screen::Failed => {
                self.status_bar(ui);
                self.failure_screen(&ctx);
            }
            Screen::Loaded => {
                let top = self.top_bar(ui);
                let nav_actions = self.nav_panel(ui);
                self.status_bar(ui);
                // The selection the view grid (step 3) and the menus read.
                let selection = self.nav.selection(&self.nav_state);
                if selection != self.selection {
                    self.selection = selection;
                    self.show_nav_selection();
                }
                let (view, action) = self.view_area(ui);
                if let Some((id, command)) = top.or(view) {
                    self.run_menu_command(&ctx, id, command);
                }
                self.run_nav_actions(&ctx, nav_actions);
                self.run_pane_action(&ctx, action);
                // The record the record panes follow.
                if let Some(record) = self.selection_record() {
                    self.set_pane_record(Some(record));
                }
                // The conflict colours of the rows the tree painted.
                self.load_visible_conflicts(&ctx);
            }
        }
        // The step-5 selection dialogs and the step-6 viewers and editors
        // draw over whichever screen the shell shows.
        self.draw_dialogs(&ctx);
        self.show_dialogs(&ctx);
        self.confirm_window(&ctx);
        self.nav_flow_question_window(&ctx);
        self.draw_prompt(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_column_is_not_a_data_column() {
        // `Pred(FocusedColumn)` of `vstView` with the focus on the label
        // column is -1: no record column, and no underflow.
        let mut form = TfrmMain::new(None, Vec::new(), true);
        form.view.state.focused_column = 0;
        assert_eq!(form.focused_view_column(), None);
        assert!(form.focused_view_cell().is_none());
        form.view.state.focused_column = 1;
        assert_eq!(form.focused_view_column(), None, "the view has no record column");
        form.view_header_column = Some(0);
        assert_eq!(form.header_view_column(), None);
        form.view_header_column = Some(1);
        assert_eq!(form.header_view_column(), None);
    }

    #[test]
    fn the_navigation_selection_fills_the_view() {
        // `vstNavSelectionChanged`: the selected records fill the view, and a
        // focused file node shows its header (`_File.Elements[0]`) -- except
        // the hardcoded executable, which has no load order slot.
        let mut form = TfrmMain::new(None, Vec::new(), true);
        form.files = vec![
            "Skyrim.esm".to_owned(),
            "SkyrimSE.exe".to_owned(),
            "Update.esm".to_owned(),
        ];
        form.file_ids = vec![0, -1, 1];
        form.selection.focused = Some(crate::selection::SelectedNode {
            kind: NodeKind::File,
            element_name: "Update.esm".to_owned(),
            file: Some("Update.esm".to_owned()),
            ..Default::default()
        });
        form.show_nav_selection();
        assert_eq!(
            form.view_selection,
            vec![ViewRecord {
                form_id: "01000000".to_owned(),
                file: "Update.esm".to_owned()
            }]
        );
        form.selection = Selection {
            nodes: vec![crate::selection::SelectedNode {
                kind: NodeKind::MainRecord,
                form_id: Some("0000000A".to_owned()),
                file: Some("Update.esm".to_owned()),
                ..Default::default()
            }],
            focused: None,
        };
        form.show_nav_selection();
        assert_eq!(
            form.view_selection,
            vec![ViewRecord {
                form_id: "0000000A".to_owned(),
                file: "Update.esm".to_owned()
            }]
        );
    }

    /// The shell nests `pnlTop`, `pnlNav`, `stbMain`, `pgMainTabs` and the
    /// central area in one root `Ui`, as eframe's `App::ui` gives it. The
    /// bottom panels must take their space before the central panel, or the
    /// tab strip and the status bar are painted over.
    #[test]
    fn the_bottom_panels_shrink_the_central_area() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1000.0, 700.0),
            )),
            ..Default::default()
        };
        let mut form = TfrmMain::new(None, Vec::new(), true);
        form.startup = Startup::Loaded;
        let mut output = ctx.run_ui(input, |ui| form.draw(ui));
        output.textures_delta.clear();
        let rect = |name: &str| {
            form.rects
                .iter()
                .find(|(panel, _)| *panel == name)
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| panic!("{name} was not drawn"))
        };
        let status = rect("stbMain");
        let tabs = rect("pgMainTabs");
        let central = rect("central");
        assert!(
            central.bottom() <= tabs.top() + 0.5,
            "the central area {central:?} runs over the tab strip {tabs:?}"
        );
        assert!(
            tabs.bottom() <= status.top() + 0.5,
            "the tab strip {tabs:?} runs over the status bar {status:?}"
        );
        assert!(tabs.height() > 0.0 && status.height() > 0.0);
    }

    /// The asks of a flow, in the order upstream asks them.
    fn flow_asks(form: &TfrmMain) -> Vec<NavAsk> {
        form.nav_flow.as_ref().expect("a flow is running").asks.clone()
    }

    #[test]
    fn a_flow_builds_its_asks_in_order() {
        // `mniNavCreateMergedPatchClick`: the name of the new file, then
        // the command.
        let mut form = TfrmMain::new(None, Vec::new(), true);
        form.run_nav_flow(NavFlow::MergedPatch {
            file: "Test.esp".to_owned(),
        });
        let asks = flow_asks(&form);
        assert_eq!(asks.len(), 1);
        assert_eq!(
            asks[0],
            NavAsk::Text {
                caption: "New Module File".to_owned(),
                message: "Filename without extension:".to_owned(),
                value: String::new(),
                require_text: true,
            }
        );
        // The first ask opens its dialog.
        assert!(matches!(form.dialog, Some(Dialog::TextPrompt(_))));
    }

    #[test]
    fn the_new_file_flow_offers_the_templates_first() {
        // `mniCreateNewFileClick`: the kind of the file, then its name.
        let mut form = TfrmMain::new(None, Vec::new(), true);
        form.run_nav_flow(NavFlow::NewFile);
        let asks = flow_asks(&form);
        assert_eq!(asks.len(), 2);
        assert_eq!(
            asks[0],
            NavAsk::Files {
                caption: "What type of module do you want to create?".to_owned(),
                items: file_templates(),
                checked: Vec::new(),
                single: true,
            }
        );
        assert!(matches!(asks[1], NavAsk::Text { .. }));
        assert!(matches!(form.dialog, Some(Dialog::FileSelect(_))));
    }

    #[test]
    fn a_cancelled_ask_ends_the_flow() {
        let mut form = TfrmMain::new(None, Vec::new(), true);
        form.run_nav_flow(NavFlow::NewFile);
        assert!(form.nav_flow.is_some());
        form.nav_flow_cancel();
        assert!(form.nav_flow.is_none());
        assert!(form.nav_question.is_none());
    }

    #[test]
    fn the_update_crc_flow_asks_the_second_question_only_after_a_yes() {
        // `mniNavUpdateCRCModGroupsClick`: the second question follows the
        // first one, and the CRCs that are missing are still updated when
        // the first answer was no.
        let mut form = TfrmMain::new(None, Vec::new(), true);
        form.run_nav_flow(NavFlow::UpdateCrc);
        assert_eq!(flow_asks(&form).len(), 4);
        form.nav_flow_answer(NavAnswer::Yes(false));
        let state = form.nav_flow.as_ref().expect("the flow waits for the modules");
        assert_eq!(state.answers, vec![NavAnswer::Yes(false), NavAnswer::Yes(true)]);
        assert_eq!(state.index, 2);
        // The caption of the module list follows the answers.
        assert_eq!(
            state.asks[2],
            NavAsk::Files {
                caption: "Please select the modules for which you want to update the current CRC in ModGroups..."
                    .to_owned(),
                items: Vec::new(),
                checked: Vec::new(),
                single: false,
            }
        );
    }

    #[test]
    fn the_caption_of_the_crc_modules_follows_the_answers() {
        assert_eq!(
            update_crc_caption(&[NavAnswer::Yes(true), NavAnswer::Yes(true)]),
            "Please select the modules for which you want to add or update the current CRC to/in ModGroup Items..."
        );
        assert_eq!(
            update_crc_caption(&[NavAnswer::Yes(true), NavAnswer::Yes(false)]),
            "Please select the modules for which you want to add the current CRC to ModGroups"
        );
        assert_eq!(
            update_crc_caption(&[NavAnswer::Yes(false), NavAnswer::Yes(true)]),
            "Please select the modules for which you want to update the current CRC in ModGroups..."
        );
    }

    #[test]
    fn the_copy_idle_flow_asks_a_model_prefix_for_every_chosen_prefix() {
        // `mniNavCopyIdleClick`: one "Model Prefix" box per chosen prefix.
        let mut form = TfrmMain::new(None, Vec::new(), true);
        form.run_nav_flow(NavFlow::CopyIdle {
            file: "Test.esp".to_owned(),
        });
        assert_eq!(flow_asks(&form).len(), 1);
        form.nav_flow_answer(NavAnswer::Files(vec![
            "assets\\idle".to_owned(),
            "assets\\other".to_owned(),
        ]));
        let asks = flow_asks(&form);
        assert_eq!(asks.len(), 4);
        assert_eq!(
            asks[1],
            NavAsk::Text {
                caption: "Model Prefix".to_owned(),
                message: "Please change the model prefix".to_owned(),
                value: "assets\\idle".to_owned(),
                require_text: true,
            }
        );
        assert_eq!(
            asks[2],
            NavAsk::Text {
                caption: "Model Prefix".to_owned(),
                message: "Please change the model prefix".to_owned(),
                value: "assets\\other".to_owned(),
                require_text: true,
            }
        );
        assert!(matches!(asks[3], NavAsk::Files { .. }));
    }

    #[test]
    fn the_delta_patch_flow_refuses_a_file_that_is_not_a_module() {
        let mut form = TfrmMain::new(None, Vec::new(), true);
        form.files = vec!["Test.esp".to_owned()];
        form.run_nav_flow(NavFlow::DeltaPatch {
            file: "Test.esp".to_owned(),
        });
        form.nav_flow_answer(NavAnswer::Path("Other\\Thing.esp".to_owned()));
        assert!(form.nav_flow.is_none());
        assert!(
            form.messages
                .lines
                .iter()
                .any(|line| line == "Delta patch can only be created for modules"),
            "the message of the refusal is written: {:?}",
            form.messages.lines
        );
    }
}
