// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (FormCreate, FormShow,
// tmrStartupTimer, DoInit's message log and source selection,
// WMUserLoaderDone, FormClose and SaveSettings, AddMessage,
// ScrollToTheLastMessage, bnMainMenuMouseDown, mniMessagesClearClick,
// mniMessagesSaveSelectedClick, mniMessagesAutoscroll, the Help buttons)
// and xEdit/xeMainForm.dfm (pnlClient, splElements, stbMain, pnlTop with
// bnMainMenu, bnBack, bnForward, lblPath and pnlBtn, pnlNav with vstNav,
// pnlRight with pgMain: tbsView, tbsReferencedBy, tbsMessages, tbsInfo and
// the three spreadsheet tabs, and the popup menus)

//! `TfrmMain`: the main window of the GUI. The form owns the shell state
//! (the startup screen, the settings, the message log and the tab), draws
//! the top bar, the navigation pane, the tabbed view area and the status
//! bar, and runs the startup load on a worker thread.
//!
//! Steps 2 to 4 fill the navigation tree, the view and the record panes;
//! this step carries their placeholders and the context menus they attach
//! to.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui;
use xedit_core::interface::globals::GameMode;
use xedit_session::filter::{
    FilterApplyRequest, FilterApplyResponse, FilterPresetDeleteRequest, FilterPresetSaveRequest, FilterPresetsRequest,
    FilterPresetsResponse, SignaturesListRequest, SignaturesListResponse,
};
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
use xedit_session::settings::{SettingsGetRequest, SettingsGetResponse, SettingsSetRequest, SettingsSetResponse};
use xedit_session::{CommandError, Session};

use crate::command::{self, CommandBridge};
use crate::dialogs::file_select_form::{self, FileSelectAction, FileSelectState};
use crate::dialogs::filter_options_form::{
    self as filter_options_form, FilterOptionsAction, FilterOptionsState, FilterQuestion,
};
use crate::dialogs::mod_group_edit_form::{self as mod_group_edit_form, ModGroupEditAction, ModGroupEditState};
use crate::dialogs::mod_group_select_form::{self as mod_group_select_form, ModGroupSelectAction, ModGroupSelectState};
use crate::dialogs::module_select_form::{self as module_select_form, ModuleSelectAction};
use crate::dialogs::selection::DialogPlacement;
use crate::menu_model::{self, MenuItem, MenuKind, MenuTree};
use crate::settings::{GuiSettings, NAV_COLUMN_WIDTHS, WindowState};
use crate::startup::{ModuleSelectMode, ModuleSelectState, Startup, application_title};
use crate::widgets::grid::{Grid, GridColumn, GridSource, GridState};
use crate::widgets::virtual_tree::{TreeCell, TreeColumn, TreeRow, TreeSource, TreeState, TreeView};
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

/// The navigation tree of step 1: the loaded files, before step 2 builds
/// the real tree (`vstNavInitNode`/`vstNavInitChildren`).
struct FilesSource {
    files: Vec<String>,
    columns: Vec<TreeColumn>,
}

impl FilesSource {
    fn new(files: Vec<String>) -> Self {
        Self {
            files,
            columns: NAV_COLUMN_WIDTHS
                .iter()
                .zip(["FormID", "EditorID", "Name"])
                .map(|(width, title)| TreeColumn::new(title, *width))
                .collect(),
        }
    }
}

impl TreeSource for FilesSource {
    fn row_count(&self) -> usize {
        self.files.len()
    }

    fn columns(&self) -> &[TreeColumn] {
        &self.columns
    }

    fn row(&self, index: usize, out: &mut TreeRow) {
        let name = &self.files[index];
        out.id = index as u64;
        out.depth = 1;
        out.icon = Some('\u{25A4}');
        out.cells = vec![TreeCell::new(""), TreeCell::new(""), TreeCell::new(name.clone())];
    }
}

/// The empty view of step 1: the two columns of `vstView` (`Labels` 250,
/// `Values` 233) and no rows before step 3 builds the record grid.
struct EmptyView {
    columns: Vec<GridColumn>,
}

impl GridSource for EmptyView {
    fn row_count(&self) -> usize {
        0
    }

    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }

    fn cell(&self, _row: usize, _column: usize, _out: &mut crate::widgets::grid::GridCell) {}
}

/// The step-5 dialogs, one at a time (they are modal upstream).
pub enum Dialog {
    ModGroupSelect(Box<ModGroupSelectState>),
    ModGroupEdit(Box<ModGroupEditState>),
    FilterOptions(Box<FilterOptionsState>),
    FileSelect(Box<FileSelectState>),
}

/// The main form.
pub struct TfrmMain {
    /// `Application.Title` and the native window data.
    pub program: PathBuf,
    pub settings: GuiSettings,
    pub startup: Startup,
    pub game: Option<GameMode>,
    pub bridge: Option<CommandBridge>,
    /// `mmoMessages.Lines`.
    pub messages: Vec<String>,
    /// `mniMessagesAutoscroll.Checked`.
    pub messages_autoscroll: bool,
    pub active_tab: Tab,
    pub nav_state: TreeState,
    pub view_state: GridState,
    pub files: Vec<String>,
    pub data_path: String,
    /// `mniBtnShrinkButtons`.
    pub shrink_buttons: bool,
    /// The radio entries of `mniViewStick*` (`Auto Top Row` by default).
    pub stick_follows_selection: bool,
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
    /// `--dialog <name>`: the dialog the harness asks to open once loaded.
    pub pending_dialog: Option<String>,
    /// `--dialog-arg <name>`: the mod group the edit dialog opens for.
    pub dialog_arg: Option<String>,
    /// The load running on its worker thread.
    load_rx: Option<Receiver<Result<Session, CommandError>>>,
    /// The progress callback's buffer while a load runs.
    progress: Option<Arc<Mutex<Vec<String>>>>,
    /// The callback that was installed before the load (restored after it).
    previous_callback: Option<xedit_core::interface::misc::ProgressCallback>,
    load_started: Instant,
    theme_applied: bool,
    window_placed: bool,
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
}

impl TfrmMain {
    /// `Application.CreateForm(TfrmMain, frmMain)`: the DFM defaults, the
    /// settings read once a game is chosen (`DoInit`), and the startup
    /// screen. A game named on the command line skips `TfrmGameSelect`.
    pub fn new(game: Option<GameMode>, plugins: Vec<String>, edit: bool) -> Self {
        let program = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("xedit-gui.exe"));
        let mut form = Self {
            program,
            settings: GuiSettings::default(),
            startup: Startup::game_select(),
            game: None,
            bridge: None,
            messages: Vec::new(),
            messages_autoscroll: true,
            active_tab: Tab::Messages,
            nav_state: TreeState::new(NAV_COLUMN_WIDTHS.to_vec()),
            view_state: GridState::new(vec![250.0, 233.0]),
            files: Vec::new(),
            data_path: String::new(),
            shrink_buttons: false,
            stick_follows_selection: false,
            game_select_error: None,
            module_error: None,
            module_select: None,
            module_bridge: None,
            dialog: None,
            edit_group_flow: false,
            pending_dialog: None,
            dialog_arg: None,
            load_rx: None,
            progress: None,
            previous_callback: None,
            load_started: Instant::now(),
            theme_applied: false,
            window_placed: false,
            logged_screen: None,
            frame_count: 0,
            edit_allowed: edit,
            settings_saved: false,
            closing: false,
        };
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
        self.theme_applied = false;
        self.add_message(&format!("{} starting session", application_title(game)));
    }

    /// `AddMessage`: one line per CRLF part, the status bar takes the last.
    pub fn add_message(&mut self, text: &str) {
        for line in text.split("\r\n") {
            self.messages.push(line.to_owned());
        }
        if self.messages_autoscroll {
            // The messages page scrolls itself to the bottom.
        }
    }

    pub fn status_text(&self) -> String {
        self.messages.last().cloned().unwrap_or_default()
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
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&buffer);
        xedit_core::interface::misc::set_progress_callback(Some(Arc::new(move |message: &str| {
            if let Ok(mut messages) = sink.lock() {
                messages.push(message.to_owned());
            }
        })));
        self.previous_callback = previous;
        self.progress = Some(buffer);
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
        let Some(buffer) = &self.progress else {
            return;
        };
        let drained: Vec<String> = buffer
            .lock()
            .map(|mut messages| messages.drain(..).collect())
            .unwrap_or_default();
        for message in drained {
            self.add_message(&message);
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
        self.progress = None;
        match result {
            Ok(session) => {
                let mut bridge = CommandBridge::new(session);
                match bridge
                    .request::<_, xedit_session::commands::SessionInfo>("session.info", &xedit_session::NoParams {})
                {
                    Ok(info) => {
                        self.data_path = info.data_path.clone();
                        self.files = info.files.clone();
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
                self.bridge = Some(bridge);
                self.startup = Startup::Loaded;
                self.active_tab = Tab::Messages;
                // `--dialog`: the screen-check harness opens one dialog of
                // step 5 without walking the menus.
                if let Some(dialog) = self.pending_dialog.take() {
                    self.open_pending_dialog(&dialog);
                }
            }
            Err(error) => {
                let message = format!("Fatal: {}", error.message);
                self.add_message(&message);
                self.startup = Startup::Failed(message);
            }
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
    fn top_bar(&mut self, ui: &mut egui::Ui) -> Option<MenuCommand> {
        let ctx = ui.ctx().clone();
        let mut command = None;
        egui::Panel::top("pnlTop").show(ui, |ui| {
            ui.horizontal(|ui| {
                // `bnMainMenu`: the hamburger opens `pmuMain`.
                ui.menu_button("\u{2630}", |ui| {
                    command = draw_menu(ui, &menu_model::PMU_MAIN, 1, self);
                })
                .response
                .on_hover_text("Main menu");
                // `bnBack` and `bnForward` (the view history of step 3).
                for (glyph, tooltip) in [("\u{25C0}", "Back"), ("\u{25B6}", "Forward")] {
                    ui.add_enabled(false, egui::Button::new(glyph))
                        .on_disabled_hover_text(format!("{tooltip}: phase 7 step 3"));
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
                    // `pmuBtnMenu`.
                    strip.response.context_menu(|ui| {
                        if ui.checkbox(&mut self.shrink_buttons, "Shrink Buttons").changed() {
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

    /// The `lblPath` text: the selected navigation row's name.
    fn selected_path_text(&self) -> String {
        self.nav_state
            .focused
            .and_then(|index| self.files.get(index).cloned())
            .unwrap_or_default()
    }

    /// `pnlNav` with `vstNav`.
    fn nav_panel(&mut self, ui: &mut egui::Ui) -> Option<MenuCommand> {
        let mut command = None;
        let panel = egui::Panel::left("pnlNav")
            .resizable(true)
            .default_size(self.settings.nav_width)
            .min_size(50.0)
            .show(ui, |ui| {
                let mut source = FilesSource::new(self.files.clone());
                let output = TreeView {
                    id: ui.scope_id().with("vstNav"),
                    source: &mut source,
                    state: &mut self.nav_state,
                    colors: &self.settings.colors,
                    header: true,
                }
                .show(ui);
                let response = ui.response();
                response.context_menu(|ui| {
                    command = draw_menu(ui, &menu_model::PMU_NAV, 2, self);
                });
                let _ = output;
            });
        self.settings.nav_width = panel.response.rect.width();
        command
    }

    /// `pgMain` with the tab strip at the bottom (`TabPosition = tpBottom`).
    fn view_area(&mut self, ui: &mut egui::Ui) -> Option<MenuCommand> {
        let mut command = None;
        egui::Panel::bottom("pgMainTabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                for tab in Tab::ALL {
                    if ui.selectable_label(self.active_tab == tab, tab.caption()).clicked() {
                        self.active_tab = tab;
                    }
                }
            });
        });
        egui::CentralPanel::default_margins().show(ui, |ui| match self.active_tab {
            Tab::View => {
                let mut source = EmptyView {
                    columns: vec![GridColumn::new("Labels", 250.0), GridColumn::new("Values", 233.0)],
                };
                let output = Grid {
                    id: ui.scope_id().with("vstView"),
                    source: &mut source,
                    state: &mut self.view_state,
                    colors: &self.settings.colors,
                    header: true,
                }
                .show(ui);
                let response = ui.response();
                response.context_menu(|ui| {
                    command = draw_menu(ui, &menu_model::PMU_VIEW, 3, self);
                });
                let _ = output;
                ui.label("No record is shown: the view tab arrives in phase 7 step 3.");
            }
            Tab::ReferencedBy => {
                let response = ui.label("The referenced-by list arrives in phase 7 step 4.").clone();
                response.context_menu(|ui| {
                    command = draw_menu(ui, &menu_model::PMU_REF_BY, 4, self);
                });
            }
            Tab::Messages => {
                let font = crate::theme::messages_font_id(&self.settings.fonts);
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .stick_to_bottom(self.messages_autoscroll)
                    .show(ui, |ui| {
                        let text = self.messages.join("\n");
                        ui.add(
                            egui::Label::new(egui::RichText::new(text).font(font.clone()))
                                .wrap_mode(egui::TextWrapMode::Extend),
                        );
                    });
                let response = ui.response();
                response.context_menu(|ui| {
                    command = draw_menu(ui, &menu_model::PMU_MESSAGES, 1, self);
                });
            }
            Tab::Information => {
                ui.label("xEdit is an advanced graphical esp editor and conflict detector.");
                ui.label(format!(
                    "{}: {}",
                    self.game.map(application_title).unwrap_or_else(|| "xEdit".to_owned()),
                    self.data_path
                ));
                ui.label("https://discord.gg/5t8RnNQ");
            }
            Tab::Weapon | Tab::Armor | Tab::Ammunition => {
                let response = ui.label("The spreadsheets arrive in phase 7 step 3.").clone();
                response.context_menu(|ui| {
                    command = draw_menu(ui, &menu_model::PMU_SPREADSHEET, 4, self);
                });
            }
        });
        command
    }

    /// `stbMain`: the last message and the progress gauge of a load.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("stbMain").show(ui, |ui| {
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
    }

    /// Runs a menu command the form handles itself.
    fn run_menu_command(&mut self, ctx: &egui::Context, command: MenuCommand) {
        match command {
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
                _ => self.add_message(&format!("{name} is not wired by this step")),
            },
            MenuCommand::Presentation(id) => match id {
                "mniMessagesClear" => self.messages.clear(),
                "mniMessagesSaveSelected" => {
                    let text = self.messages.join("\r\n");
                    ctx.copy_text(text);
                    self.add_message("The message log is on the clipboard");
                }
                "mniMessagesAutoscroll" => self.messages_autoscroll = !self.messages_autoscroll,
                "mniViewColumnWidthStandard" => {
                    self.settings.view.column_width = crate::settings::ColumnWidthMode::Standard
                }
                "mniViewColumnWidthFitAll" => {
                    self.settings.view.column_width = crate::settings::ColumnWidthMode::FitAll
                }
                "mniViewColumnWidthFitText" => {
                    self.settings.view.column_width = crate::settings::ColumnWidthMode::FitText
                }
                "mniViewColumnWidthFitSmart" => {
                    self.settings.view.column_width = crate::settings::ColumnWidthMode::FitSmart
                }
                "mniViewStickAuto" => self.stick_follows_selection = false,
                "mniViewStickSelected" => self.stick_follows_selection = true,
                "mniNavHeaderFilesDefault" => {
                    self.settings.view.files_sort = crate::settings::FilesSort::AsSelected
                }
                "mniNavHeaderFilesLoadOrder" => {
                    self.settings.view.files_sort = crate::settings::FilesSort::LoadOrder
                }
                "mniNavHeaderFilesFileName" => {
                    self.settings.view.files_sort = crate::settings::FilesSort::FileName
                }
                "mniNavHeaderINFObyPreviousINFO" => {
                    self.settings.view.info_sort = crate::settings::InfoSort::ByPreviousInfo
                }
                "mniNavHeaderINFObyFormID" => {
                    self.settings.view.info_sort = crate::settings::InfoSort::ByFormId
                }
                "mniBtnShrinkButtons" => self.shrink_buttons = !self.shrink_buttons,
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
        self.dialog_command(|bridge| {
            bridge.request::<_, SettingsGetResponse>(
                "settings.get",
                &SettingsGetRequest {
                    section: Some(section.to_owned()),
                    key: None,
                    value_type: None,
                    default: None,
                },
            )
        })
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
                        self.add_message(&format!(
                            "{} files selected{}",
                            items.len(),
                            if backup { ", backups are made" } else { "" }
                        ));
                        self.save_dialog_placement("frmFileSelect", state.rect);
                    }
                    Some(FileSelectAction::Cancelled) => {
                        self.save_dialog_placement("frmFileSelect", state.rect);
                    }
                    None => self.dialog = Some(Dialog::FileSelect(state)),
                }
            }
        }
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
        match self.dialog_command(|bridge| bridge.request::<_, FilterApplyResponse>("filter.apply", &request)) {
            Ok(response) => {
                for message in &response.messages {
                    self.add_message(message);
                }
                self.add_message(&format!("[Pass 1] Processed Records: {}", response.pass1));
                self.add_message(&format!("[Pass 2] Processed Records: {}", response.pass2));
                self.add_message(&format!("Remaining unfiltered nodes: {}", response.unfiltered));
            }
            Err(error) => self.add_message(&format!("Fatal: {}", error.message)),
        }
        self.save_dialog_placement("frmFilterOptions", rect);
    }
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

/// Draws a popup menu from the `xeMainForm.dfm` model. An item whose step
/// is later than `step` is disabled with the step and the command in its
/// tooltip, so every upstream item is visible from the first step.
fn draw_menu(ui: &mut egui::Ui, tree: &'static MenuTree, step: u8, form: &mut TfrmMain) -> Option<MenuCommand> {
    let mut chosen = None;
    draw_items(ui, tree.items, step, form, &mut chosen);
    chosen
}

fn draw_items(
    ui: &mut egui::Ui,
    items: &'static [MenuItem],
    step: u8,
    form: &mut TfrmMain,
    chosen: &mut Option<MenuCommand>,
) {
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
                    draw_items(ui, item.children, step, form, chosen);
                });
            }
            MenuKind::Action => {
                let later = item.step.is_some_and(|item_step| item_step > step);
                let enabled = item.enabled && !later;
                let mut button = egui::Button::new(item.caption).selected(item.checked);
                if let Some(shortcut) = item.shortcut {
                    button = button.shortcut_text(shortcut);
                }
                let response = ui.add_enabled(enabled, button);
                if later {
                    let tooltip = match (item.step, item.command) {
                        (Some(item_step), Some(command)) => {
                            format!("phase 7 step {item_step}: {command}")
                        }
                        (Some(item_step), None) => format!("phase 7 step {item_step}"),
                        _ => String::new(),
                    };
                    response.on_disabled_hover_text(tooltip);
                } else if response.clicked() {
                    let command = crate::widgets::menu_command(item);
                    match command {
                        MenuCommand::Presentation(_) => {
                            form.run_menu_command(ui.ctx(), command);
                        }
                        other => {
                            // The form runs it after the menu closes, so a
                            // command that rebuilds a panel is safe.
                            *chosen = Some(other);
                        }
                    }
                    ui.close();
                }
            }
        }
    }
}

impl eframe::App for TfrmMain {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
        self.apply_theme(&ctx);

        // The native window: the title and the icon of the chosen game.
        if let Some(game) = self.game {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(application_title(game)));
            if let Some(icon) = crate::icon::icon_for_game(game) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(icon))));
            }
        }
        if !self.window_placed {
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
        if matches!(self.startup, Startup::Loading) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // `FormClose`: the placement and the splitter are written once, when
        // the window is asked to close.
        if ctx.input(|input| input.viewport().close_requested()) && !self.settings_saved {
            self.settings_saved = true;
            self.save_settings(&ctx);
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
            crate::log_screen(screen_name, &self.messages);
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
                let nav = self.nav_panel(ui);
                self.status_bar(ui);
                let view = self.view_area(ui);
                if let Some(command) = top.or(nav).or(view) {
                    self.run_menu_command(&ctx, command);
                }
            }
        }
        self.draw_dialogs(&ctx);
    }
}
