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
use xedit_session::{CommandError, Session};

use crate::command::{self, CommandBridge};
use crate::menu_model::{self, MenuItem, MenuKind, MenuTree};
use crate::settings::{GuiSettings, NAV_COLUMN_WIDTHS, WindowState};
use crate::startup::{ModuleSelectState, Startup, application_title};
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
    /// The load running on its worker thread.
    load_rx: Option<Receiver<Result<Session, CommandError>>>,
    /// The progress callback's buffer while a load runs.
    progress: Option<Arc<Mutex<Vec<String>>>>,
    /// The callback that was installed before the load (restored after it).
    previous_callback: Option<xedit_core::interface::misc::ProgressCallback>,
    load_started: Instant,
    theme_applied: bool,
    window_placed: bool,
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
            load_rx: None,
            progress: None,
            previous_callback: None,
            load_started: Instant::now(),
            theme_applied: false,
            window_placed: false,
            edit_allowed: edit,
            settings_saved: false,
            closing: false,
        };
        if let Some(game) = game {
            form.choose_game(game);
            if plugins.is_empty() {
                // `DoInit`'s source selection: the module dialog.
                form.startup = Startup::module_select(game);
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
        if let Some(rect) = ctx.input(|input| input.viewport().inner_rect) {
            self.settings.window.width = rect.width().round() as i32;
            self.settings.window.height = rect.height().round() as i32;
        }
        if let Some(rect) = ctx.input(|input| input.viewport().outer_rect) {
            self.settings.window.left = rect.left().round() as i32;
            self.settings.window.top = rect.top().round() as i32;
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
            MenuCommand::Session(name) => {
                self.add_message(&format!("{name} is not wired by this step"));
            }
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
                self.startup = Startup::module_select(game);
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
        let chosen = match &mut self.startup {
            Startup::ModuleSelect(state) => crate::dialogs::module_select_form::show(ctx, state, &mut self.nav_state),
            _ => return,
        };
        match chosen {
            Some(crate::dialogs::module_select_form::ModuleSelectAction::Ok(plugins)) => {
                self.start_load(plugins);
            }
            Some(crate::dialogs::module_select_form::ModuleSelectAction::Cancelled) => {
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
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
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                self.settings.window.width as f32,
                self.settings.window.height as f32,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                self.settings.window.left as f32,
                self.settings.window.top as f32,
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
    }
}
