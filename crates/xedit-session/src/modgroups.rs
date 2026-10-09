// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeMainForm.pas (mniViewModGroupsReloadClick,
// mniNavCreateModGroupClick, mniNavEditModGroupClick,
// mniNavDeleteModGroupsClick, mniNavUpdateCRCModGroupsClick,
// LoadModGroupsSelection, SaveModGroupsSelection, the mod groups of
// NodeDatasForMainRecord), xEdit/xeModGroupEditForm.pas (the CRC32
// question of ShowModal, CheckState), xEdit/xeInit.pas (xeSettingsFileName,
// wbModGroupFileName)

//! `modgroups.*`: the mod groups of the loaded plugins (`wbModGroups`,
//! ported in `xedit_loadorder::mod_groups`) and the GUI's handling of them
//! as session commands: list and show them, choose the active ones (the
//! selection xEdit keeps in its settings file), create, edit and delete
//! them, and add the current CRC32s of the modules to their items. The
//! dialogs of the GUI are parameters; a command that changes a mod group
//! file ends with the reload the GUI runs after it (the files read again,
//! their validation messages, the saved selection written again as the
//! GUI's selection dialog leaves it when it is confirmed).
//!
//! `conflicts.list` and `records.compare` take the mod groups to activate
//! ([`ModGroupChoice`]); [`ModGroupFilter`] is the part of
//! `NodeDatasForMainRecord` that leaves their hidden records out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use xedit_analysis::conflict::ModGroupFilter as ConflictModGroupFilter;
use xedit_core::implementation::MainRecordImpl;
use xedit_core::interface::Element;
use xedit_core::interface::globals::{GameMode, app_name, data_path, game_mode, game_name};
use xedit_loadorder::ini_files::{MemIniFile, comma_text, load_strings, parse_comma_text, same_text, save_strings};
use xedit_loadorder::load_order::Modules;
use xedit_loadorder::mod_groups::{
    Activation, ModGroup, ModGroupItem, ModGroupRef, ModGroups, mod_group_file_name, set_mod_group_file_name,
    write_group_section,
};

use crate::{CommandError, Registry, Session};

static SETTINGS_FILE: RwLock<String> = RwLock::new(String::new());

/// The options of the CLI: the program's own mod group file (xEdit's
/// `wbModGroupFileName`) and the settings file that keeps the selection
/// (`xeSettingsFileName`); `None` keeps xEdit's default.
pub fn set_file_options(mod_groups_file: Option<&str>, settings: Option<&str>) {
    set_mod_group_file_name(mod_groups_file.unwrap_or(""));
    *SETTINGS_FILE.write().unwrap() = settings.unwrap_or("").to_owned();
}

/// The folder of the program, with a trailing separator (`wbProgramPath`).
fn program_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_owned))
        .map(|dir| format!("{}\\", dir.display()))
        .unwrap_or_default()
}

/// `wbAppName + wbToolName` of the edit mode.
fn app_tool_name() -> String {
    format!("{}Edit", app_name())
}

/// Port of `wbModGroupFileName` (`xeInit`): `<AppName><ToolName>.modgroups`
/// next to the program, unless the CLI names another file.
pub fn global_file() -> PathBuf {
    let name = mod_group_file_name();
    if !name.is_empty() {
        return PathBuf::from(name);
    }
    PathBuf::from(format!("{}{}.modgroups", program_path(), app_tool_name()))
}

/// Port of `xeSettingsFileName` (`xeInit`): `<AppName><ToolName>.ini` next
/// to the program when it exists, else `Plugins.<app>viewsettings` next to
/// the game's `Plugins.txt` in the local application data, unless the CLI
/// names another file.
pub fn settings_file() -> PathBuf {
    let name = SETTINGS_FILE.read().unwrap().clone();
    if !name.is_empty() {
        return PathBuf::from(name);
    }
    let next_to_program = PathBuf::from(format!("{}{}.ini", program_path(), app_tool_name()));
    if next_to_program.is_file() {
        return next_to_program;
    }
    let game_name2 = match game_mode() {
        GameMode::gmSSE => "Skyrim Special Edition".to_owned(),
        GameMode::gmTES5VR => "Skyrim VR".to_owned(),
        GameMode::gmEnderalSE => "Enderal Special Edition".to_owned(),
        GameMode::gmFO4VR => "Fallout4VR".to_owned(),
        // Fallout 76 keeps its plugin list under `wbGameName`.
        _ => game_name(),
    };
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    PathBuf::from(format!(
        "{local}\\{game_name2}\\Plugins.{}viewsettings",
        app_name().to_lowercase()
    ))
}

fn io_error(error: std::io::Error, path: &Path) -> CommandError {
    CommandError::new("io", format!("{}: {error}", path.display()))
}

/// Reads the mod group files of the loaded plugins again
/// (`wbReloadModGroups`).
fn load_mod_groups(session: &Session) -> Result<ModGroups, CommandError> {
    session.mode()?;
    let files = xedit_core::implementation::masters::loaded_files();
    let data = data_path();
    let modules = Modules::load(&data, &files);
    let global = global_file();
    ModGroups::load(modules, &data, &global).map_err(|error| io_error(error, &global))
}

/// Port of `LoadModGroupsSelection`: the names of the selection in the
/// settings file (`[ModGroups] Selection`, a comma text).
fn load_selection() -> Result<Vec<String>, CommandError> {
    let path = settings_file();
    let ini = MemIniFile::open(&path).map_err(|error| io_error(error, &path))?;
    Ok(parse_comma_text(&ini.read_string("ModGroups", "Selection", "")))
}

/// Port of `SaveModGroupsSelection`: the names of the selected groups as a
/// comma text in the settings file, which is written back as a whole.
fn save_selection(names: &[String]) -> Result<(), CommandError> {
    let path = settings_file();
    let mut ini = MemIniFile::open(&path).map_err(|error| io_error(error, &path))?;
    ini.write_string("ModGroups", "Selection", &comma_text(names));
    ini.update_file().map_err(|error| io_error(error, &path))
}

/// The groups of `list` whose name is one of `names`, without regard to
/// case (`TStringList.Find` in `LoadModGroupsSelection`).
fn tagged(mod_groups: &ModGroups, list: &[ModGroupRef], names: &[String]) -> Vec<ModGroupRef> {
    list.iter()
        .copied()
        .filter(|&group| names.iter().any(|name| same_text(name, &mod_groups.group(group).name)))
        .collect()
}

/// Which mod groups a comparison activates.
#[derive(Deserialize, JsonSchema, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ModGroupChoice {
    /// Activate the valid mod groups of these names (the GUI's "Which
    /// ModGroups do you want to activate?").
    #[serde(default)]
    pub mod_groups: Vec<String>,
    /// Activate every valid mod group, as xEdit does with `-autoload` or
    /// `-quickshowconflicts`.
    #[serde(default)]
    pub all_mod_groups: bool,
    /// Activate the valid mod groups of the selection saved in xEdit's
    /// settings file (what the GUI offers checked).
    #[serde(default)]
    pub saved_mod_groups: bool,
}

impl ModGroupChoice {
    fn is_empty(&self) -> bool {
        self.mod_groups.is_empty() && !self.all_mod_groups && !self.saved_mod_groups
    }
}

/// The groups of a choice among the valid ones (`wbModGroupsByName`); a
/// name that is not a valid group is an error.
fn chosen(mod_groups: &ModGroups, choice: &ModGroupChoice) -> Result<Vec<ModGroupRef>, CommandError> {
    let valid = mod_groups.by_name(true);
    if choice.all_mod_groups {
        return Ok(valid);
    }
    for name in &choice.mod_groups {
        if !valid
            .iter()
            .any(|&group| same_text(&mod_groups.group(group).name, name))
        {
            return Err(unknown_group(name, true));
        }
    }
    let mut names = choice.mod_groups.clone();
    if choice.saved_mod_groups {
        names.extend(load_selection()?);
    }
    Ok(tagged(mod_groups, &valid, &names))
}

fn unknown_group(name: &str, valid: bool) -> CommandError {
    CommandError::new(
        "unknown_mod_group",
        if valid {
            format!("there is no valid mod group named \"{name}\"")
        } else {
            format!("there is no mod group named \"{name}\"")
        },
    )
}

/// The records a module hides while mod groups are active.
#[derive(Serialize, JsonSchema, Clone)]
pub struct ModGroupHides {
    /// The module whose records hide.
    pub file: String,
    /// The earlier modules whose records it hides (`miModGroupTargets`),
    /// the nearest first.
    pub hides: Vec<String>,
}

fn hides_report(mod_groups: &ModGroups, activation: &Activation) -> Vec<ModGroupHides> {
    let modules = &mod_groups.modules.modules;
    activation
        .targets
        .iter()
        .enumerate()
        .filter(|(_, targets)| !targets.is_empty())
        .map(|(i, targets)| ModGroupHides {
            file: modules[i].name.clone(),
            hides: targets.iter().map(|&target| modules[target].name.clone()).collect(),
        })
        .collect()
}

/// The mod groups' part of `NodeDatasForMainRecord` for the conflict code.
struct ModGroupFilter {
    /// The module of each loaded file, by the upper case file name.
    modules: HashMap<String, usize>,
    activation: Activation,
}

impl ConflictModGroupFilter for ModGroupFilter {
    fn filter(&self, records: &mut Vec<Arc<MainRecordImpl>>) {
        let modules: Vec<Option<usize>> = records
            .iter()
            .map(|record| {
                record
                    .get_file()
                    .and_then(|file| self.modules.get(&file.get_name().to_uppercase()).copied())
            })
            .collect();
        let keep = self.activation.keep(&modules);
        let mut index = 0;
        records.retain(|_| {
            index += 1;
            keep[index - 1]
        });
    }
}

/// The activated mod groups of a comparison.
pub struct ActiveModGroups {
    /// The filter of the conflict code; `None` when no module hides another
    /// (`ModGroupsEnabled` is off).
    pub filter: Option<Arc<dyn ConflictModGroupFilter>>,
    /// The names of the activated groups.
    pub names: Vec<String>,
    pub hides: Vec<ModGroupHides>,
}

/// Activates the chosen mod groups for a comparison (`Activate`, and
/// `ModGroupsEnabled := ModGroupsExist`).
pub fn activate(session: &Session, choice: &ModGroupChoice) -> Result<ActiveModGroups, CommandError> {
    if choice.is_empty() {
        return Ok(ActiveModGroups {
            filter: None,
            names: Vec::new(),
            hides: Vec::new(),
        });
    }
    let mod_groups = load_mod_groups(session)?;
    let groups = chosen(&mod_groups, choice)?;
    let activation = mod_groups.activate(&groups);
    let hides = hides_report(&mod_groups, &activation);
    let names = groups
        .iter()
        .map(|&group| mod_groups.group(group).name.clone())
        .collect();
    let filter: Option<Arc<dyn ConflictModGroupFilter>> = activation.exist.then(|| {
        let modules = mod_groups
            .modules
            .modules
            .iter()
            .enumerate()
            .filter_map(|(i, module)| module.file.as_ref().map(|file| (file.get_name().to_uppercase(), i)))
            .collect();
        Arc::new(ModGroupFilter { modules, activation }) as Arc<dyn ConflictModGroupFilter>
    });
    Ok(ActiveModGroups { filter, names, hides })
}

/// One item of a mod group.
#[derive(Serialize, JsonSchema)]
pub struct ItemInfo {
    /// The item as the file has it (`TwbModGroupItem.ToString`).
    pub line: String,
    pub file_name: String,
    /// `+`: the group does not need the module.
    pub optional: bool,
    /// Its records can be hidden by a source below it.
    pub target: bool,
    /// Its records hide the targets above it.
    pub source: bool,
    /// `!`: the group is invalid while the module is loaded.
    pub forbidden: bool,
    /// `}`: the load order of the module is not checked.
    pub ignore_load_order: bool,
    /// `{`: the load order is not checked among the items of a block.
    pub ignore_load_order_in_block: bool,
    /// The CRC32s the module's file must have, as eight hexadecimal digits;
    /// any file when empty.
    pub crc32s: Vec<String>,
    /// The load order of the module, when it is loaded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_order: Option<i32>,
    /// The CRC32 of the module's file, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_crc32: Option<String>,
    /// The module is loaded and has one of the CRC32s (`mgifHasFile`).
    pub has_file: bool,
    /// The item does not make the group invalid (`mgifValid`).
    pub valid: bool,
}

/// One mod group.
#[derive(Serialize, JsonSchema)]
pub struct ModGroupInfo {
    pub name: String,
    /// The `.modgroups` file it is in.
    pub file: String,
    pub valid: bool,
    /// Whether the saved selection has it.
    pub selected: bool,
    /// The validation messages of its items and of the group.
    pub messages: Vec<String>,
    pub items: Vec<ItemInfo>,
}

fn item_info(mod_groups: &ModGroups, item: &ModGroupItem) -> ItemInfo {
    let module = item.module.map(|index| &mod_groups.modules.modules[index]);
    ItemInfo {
        line: item.to_string(),
        file_name: item.file_name.clone(),
        optional: item.flags.optional,
        target: item.flags.is_target,
        source: item.flags.is_source,
        forbidden: item.flags.forbidden,
        ignore_load_order: item.flags.ignore_load_order_always,
        ignore_load_order_in_block: item.flags.ignore_load_order_in_block,
        crc32s: item.crc32s.iter().map(|crc| format!("{crc:08X}")).collect(),
        load_order: module
            .filter(|module| module.has_file())
            .map(|module| module.load_order),
        current_crc32: module
            .and_then(|module| module.get_crc32())
            .map(|crc| format!("{crc:08X}")),
        has_file: item.flags.has_file,
        valid: item.flags.valid,
    }
}

fn group_info(mod_groups: &ModGroups, group: ModGroupRef, selection: &[String]) -> ModGroupInfo {
    let mod_group = mod_groups.group(group);
    ModGroupInfo {
        name: mod_group.name.clone(),
        file: mod_groups.files[group.0].file_name.display().to_string(),
        valid: mod_group.valid,
        selected: selection.iter().any(|name| same_text(name, &mod_group.name)),
        messages: mod_group
            .validation_messages()
            .iter()
            .map(ToString::to_string)
            .collect(),
        items: mod_group.items.iter().map(|item| item_info(mod_groups, item)).collect(),
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListRequest {
    /// Also list the invalid mod groups and those of files whose modules
    /// are not loaded (`wbModGroupsByName(False)`).
    #[serde(default)]
    pub all: bool,
}

#[derive(Serialize, JsonSchema)]
pub struct ListResponse {
    /// The mod groups by name.
    pub mod_groups: Vec<ModGroupInfo>,
    /// The mod group files that were read, in the order they were found.
    pub files: Vec<String>,
    /// The settings file the selection is kept in.
    pub settings: String,
}

fn modgroups_list(session: &mut Session, request: ListRequest) -> Result<ListResponse, CommandError> {
    let mod_groups = load_mod_groups(session)?;
    let selection = load_selection()?;
    Ok(ListResponse {
        mod_groups: mod_groups
            .by_name(!request.all)
            .into_iter()
            .map(|group| group_info(&mod_groups, group, &selection))
            .collect(),
        files: mod_groups
            .files
            .iter()
            .map(|file| file.file_name.display().to_string())
            .collect(),
        settings: settings_file().display().to_string(),
    })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowRequest {
    /// The name of the mod group.
    pub name: String,
    /// The `.modgroups` file (a path or a file name), when several files
    /// have a group of the name.
    pub file: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct ShowResponse {
    pub mod_group: ModGroupInfo,
    /// The section as the file would have it (`ToStrings`).
    pub lines: Vec<String>,
}

/// The one group of a name among all groups (`wbModGroupsByName(False)`),
/// in the file when one is given.
fn find_group(mod_groups: &ModGroups, name: &str, file: Option<&str>) -> Result<ModGroupRef, CommandError> {
    let found: Vec<ModGroupRef> = mod_groups
        .by_name(false)
        .into_iter()
        .filter(|&group| same_text(&mod_groups.group(group).name, name))
        .filter(|&group| file.is_none_or(|file| file_matches(&mod_groups.files[group.0].file_name, file)))
        .collect();
    match found.as_slice() {
        [group] => Ok(*group),
        [] => Err(unknown_group(name, false)),
        _ => Err(CommandError::new(
            "ambiguous_mod_group",
            format!("several mod groups are named \"{name}\": pass file"),
        )),
    }
}

fn file_matches(path: &Path, file: &str) -> bool {
    same_text(&path.to_string_lossy(), file)
        || path
            .file_name()
            .is_some_and(|name| same_text(&name.to_string_lossy(), file))
}

fn modgroups_show(session: &mut Session, request: ShowRequest) -> Result<ShowResponse, CommandError> {
    let mod_groups = load_mod_groups(session)?;
    let group = find_group(&mod_groups, &request.name, request.file.as_deref())?;
    let selection = load_selection()?;
    Ok(ShowResponse {
        mod_group: group_info(&mod_groups, group, &selection),
        lines: mod_groups.group(group).to_strings(),
    })
}

/// What the reload after a change leaves (`mniViewModGroupsReloadClick`
/// with its dialog confirmed).
#[derive(Serialize, JsonSchema, Default)]
pub struct Reload {
    /// The lines the reload writes to the message log
    /// (`ShowValidationMessages`): each mod group with messages, and its
    /// messages.
    pub validation_messages: Vec<String>,
    /// The valid mod groups the selection now has, as saved.
    pub selected: Vec<String>,
    /// What the selected groups hide once active.
    pub hides: Vec<ModGroupHides>,
}

/// Port of `mniViewModGroupsReloadClick` with its selection dialog
/// confirmed as shown: the mod group files read again, their validation
/// messages, the valid groups of the saved selection (and `new_name`, the
/// group just created) selected and the selection saved (an empty one when
/// there is no valid group). A dry run saves nothing.
fn reload(session: &Session, new_name: Option<&str>, dry_run: bool) -> Result<Reload, CommandError> {
    let mod_groups = load_mod_groups(session)?;
    let validation_messages = mod_groups.validation_messages(&mod_groups.by_name(false));
    let valid = mod_groups.by_name(true);
    let mut names = load_selection()?;
    names.extend(new_name.map(str::to_owned));
    let selected = tagged(&mod_groups, &valid, &names);
    let selected_names: Vec<String> = selected
        .iter()
        .map(|&group| mod_groups.group(group).name.clone())
        .collect();
    if !dry_run {
        save_selection(&selected_names)?;
    }
    let activation = mod_groups.activate(&selected);
    Ok(Reload {
        validation_messages,
        selected: selected_names,
        hides: hides_report(&mod_groups, &activation),
    })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectRequest {
    /// Select the valid mod groups of these names.
    #[serde(default)]
    pub names: Vec<String>,
    /// Keep the valid groups of the saved selection selected too.
    #[serde(default)]
    pub saved: bool,
    /// Select every valid mod group.
    #[serde(default)]
    pub all: bool,
    /// Report the selection and what it hides, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

fn modgroups_select(session: &mut Session, request: SelectRequest) -> Result<Reload, CommandError> {
    let mod_groups = load_mod_groups(session)?;
    let validation_messages = mod_groups.validation_messages(&mod_groups.by_name(false));
    let choice = ModGroupChoice {
        mod_groups: request.names,
        all_mod_groups: request.all,
        saved_mod_groups: request.saved,
    };
    let selected = chosen(&mod_groups, &choice)?;
    let names: Vec<String> = selected
        .iter()
        .map(|&group| mod_groups.group(group).name.clone())
        .collect();
    if !request.dry_run {
        save_selection(&names)?;
    }
    let activation = mod_groups.activate(&selected);
    Ok(Reload {
        validation_messages,
        selected: names,
        hides: hides_report(&mod_groups, &activation),
    })
}

/// The response of a command that changes mod group files.
#[derive(Serialize, JsonSchema)]
pub struct ChangeResponse {
    /// The files written (or that a dry run would write).
    pub files: Vec<String>,
    /// What the command did, as the GUI says it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub dry_run: bool,
    /// The reload that follows the change in the GUI; none when the GUI
    /// stops before it.
    #[serde(flatten)]
    pub reload: Option<Reload>,
}

/// The items of `lines`, each a line of a mod group file; one that is not
/// an item is an error.
fn parse_items(lines: &[String], modules: &Modules) -> Result<Vec<ModGroupItem>, CommandError> {
    lines
        .iter()
        .map(|line| {
            ModGroupItem::load(line, modules)
                .ok_or_else(|| CommandError::new("invalid_params", format!("\"{line}\" is not a mod group item")))
        })
        .collect()
}

/// Port of the CRC32 question of `TfrmModGroupEdit.ShowModal`: the items
/// with CRC32s that lack their module's current one get it when
/// `add_current_crcs` (the dialog's default is No).
fn add_current_crcs(mod_groups: &ModGroups, items: &mut [ModGroupItem]) {
    for item in items {
        if item.crc32s.is_empty() {
            continue;
        }
        if let Some(crc) = item
            .module
            .and_then(|index| mod_groups.modules.modules[index].get_crc32())
            && !item.crc32s.contains(&crc)
        {
            item.crc32s.push(crc);
        }
    }
}

/// `TfrmModGroupEdit.CheckState`.
fn check_state(group: &ModGroup) -> Result<(), CommandError> {
    if group.name.is_empty() {
        return Err(CommandError::new("invalid_params", "Name is empty"));
    }
    if group.items.len() < 2 {
        return Err(CommandError::new(
            "invalid_params",
            "ModGroup needs to contain at least 2 items",
        ));
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    /// The name of the new mod group.
    pub name: String,
    /// The loaded modules of the group, in order: the first is a target
    /// only, the last a source only, the others both, as the GUI's "Create
    /// ModGroup" makes them.
    #[serde(default)]
    pub modules: Vec<String>,
    /// The items of the group as lines of a mod group file
    /// (`[flags]file[:crc32,...]`), in place of `modules`.
    #[serde(default)]
    pub items: Vec<String>,
    /// Give each module's item its current CRC32 ("Do you want to include
    /// the current CRC32s?").
    #[serde(default)]
    pub include_crcs: bool,
    /// Add the current CRC32 to the items whose CRC32s lack it (the
    /// question of the edit dialog).
    #[serde(default)]
    pub add_current_crcs: bool,
    /// The loaded module of the group whose `.modgroups` file gets the
    /// group; the first loaded one when omitted.
    pub file: Option<String>,
    /// Report the file that would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

fn modgroups_create(session: &mut Session, request: CreateRequest) -> Result<ChangeResponse, CommandError> {
    let mod_groups = load_mod_groups(session)?;
    let modules = &mod_groups.modules;
    let mut group = ModGroup {
        file: 0,
        valid: false,
        name: request.name.clone(),
        items: Vec::new(),
        valid_msgs: Vec::new(),
    };
    match (request.modules.is_empty(), request.items.is_empty()) {
        (false, true) => {
            let count = request.modules.len();
            for (i, name) in request.modules.iter().enumerate() {
                let index = modules
                    .by_name(name)
                    .filter(|&index| modules.modules[index].has_file())
                    .ok_or_else(|| CommandError::new("unknown_file", format!("{name} is not loaded")))?;
                let module = &modules.modules[index];
                let mut item = ModGroupItem::load(&module.name, modules).expect("a module name is an item");
                item.flags.is_target = i + 1 < count;
                item.flags.is_source = i > 0;
                if request.include_crcs
                    && let Some(crc) = module.get_crc32()
                {
                    item.crc32s = vec![crc];
                }
                group.items.push(item);
            }
        }
        (true, false) => group.items = parse_items(&request.items, modules)?,
        _ => {
            return Err(CommandError::new("invalid_params", "pass either modules or items"));
        }
    }
    if request.add_current_crcs {
        add_current_crcs(&mod_groups, &mut group.items);
    }
    check_state(&group)?;
    // "In which module's .modgroups file should the new ModGroup be
    // stored?": one of the group's loaded modules.
    let candidates: Vec<usize> = group
        .items
        .iter()
        .filter_map(|item| item.module)
        .filter(|&index| modules.modules[index].has_file())
        .collect();
    let target = match &request.file {
        Some(name) => candidates
            .iter()
            .copied()
            .find(|&index| same_text(&modules.modules[index].name, name))
            .ok_or_else(|| {
                CommandError::new(
                    "invalid_params",
                    format!("{name} is not a loaded module of the mod group"),
                )
            })?,
        None => *candidates
            .first()
            .ok_or_else(|| CommandError::new("invalid_params", "the mod group has no loaded module"))?,
    };
    let file_name = PathBuf::from(format!(
        "{}{}",
        data_path(),
        change_file_ext(&modules.modules[target].name, ".modgroups")
    ));
    if !request.dry_run {
        // `TStringList.LoadFromFile`, an empty line before the group unless
        // the file ends with one, `AddStrings(ToStrings)`, `SaveToFile` in
        // the encoding the file was read in.
        let (mut lines, encoding) = if file_name.is_file() {
            let (lines, encoding) = load_strings(&file_name).map_err(|error| io_error(error, &file_name))?;
            (lines, Some(encoding))
        } else {
            (Vec::new(), None)
        };
        if lines.last().is_some_and(|line| !line.is_empty()) {
            lines.push(String::new());
        }
        lines.extend(group.to_strings());
        save_strings(&file_name, &lines, encoding).map_err(|error| io_error(error, &file_name))?;
    }
    Ok(ChangeResponse {
        files: vec![file_name.display().to_string()],
        message: None,
        dry_run: request.dry_run,
        reload: Some(reload(session, Some(&request.name), request.dry_run)?),
    })
}

/// `ChangeFileExt`.
fn change_file_ext(name: &str, extension: &str) -> String {
    match name.rfind(['.', '\\', '/', ':']) {
        Some(at) if name[at..].starts_with('.') => format!("{}{extension}", &name[..at]),
        _ => format!("{name}{extension}"),
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditRequest {
    /// The name of the mod group.
    pub name: String,
    /// The `.modgroups` file (a path or a file name), when several files
    /// have a group of the name.
    pub file: Option<String>,
    /// The new name of the group.
    pub new_name: Option<String>,
    /// The new items of the group as lines of a mod group file
    /// (`[flags]file[:crc32,...]`); the items stay when omitted.
    pub items: Option<Vec<String>>,
    /// Add the current CRC32 to the items whose CRC32s lack it (the
    /// question of the edit dialog; its default is No).
    #[serde(default)]
    pub add_current_crcs: bool,
    /// Report the file that would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

fn modgroups_edit(session: &mut Session, request: EditRequest) -> Result<ChangeResponse, CommandError> {
    let mod_groups = load_mod_groups(session)?;
    let reference = find_group(&mod_groups, &request.name, request.file.as_deref())?;
    let original = mod_groups.group(reference);
    let mut group = original.clone();
    if request.add_current_crcs {
        add_current_crcs(&mod_groups, &mut group.items);
    }
    let file_name = mod_groups.files[reference.0].file_name.clone();
    // The dialog is not shown for a group of fewer than two items, and the
    // command ends there.
    if original.items.len() < 2 {
        return Ok(ChangeResponse {
            files: Vec::new(),
            message: Some(format!(
                "ModGroup \"{}\" has fewer than 2 items and can not be edited",
                original.name
            )),
            dry_run: request.dry_run,
            reload: None,
        });
    }
    if let Some(name) = &request.new_name {
        group.name = name.clone();
    }
    if let Some(items) = &request.items {
        group.items = parse_items(items, &mod_groups.modules)?;
    }
    check_state(&group)?;
    if !request.dry_run {
        write_group_section(&file_name, &original.name, &group.to_strings())
            .map_err(|error| io_error(error, &file_name))?;
    }
    Ok(ChangeResponse {
        files: vec![file_name.display().to_string()],
        message: None,
        dry_run: request.dry_run,
        reload: Some(reload(session, None, request.dry_run)?),
    })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteRequest {
    /// The names of the mod groups to delete; every group of a name.
    pub names: Vec<String>,
    /// Delete only the groups of this `.modgroups` file (a path or a file
    /// name).
    pub file: Option<String>,
    /// Report the files that would be written, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

fn modgroups_delete(session: &mut Session, request: DeleteRequest) -> Result<ChangeResponse, CommandError> {
    let mod_groups = load_mod_groups(session)?;
    let all = mod_groups.by_name(false);
    let mut selected = Vec::new();
    for name in &request.names {
        let found: Vec<ModGroupRef> = all
            .iter()
            .copied()
            .filter(|&group| same_text(&mod_groups.group(group).name, name))
            .filter(|&group| {
                request
                    .file
                    .as_deref()
                    .is_none_or(|file| file_matches(&mod_groups.files[group.0].file_name, file))
            })
            .collect();
        if found.is_empty() {
            return Err(unknown_group(name, false));
        }
        selected.extend(found);
    }
    if selected.is_empty() {
        return Err(CommandError::new("invalid_params", "No ModGroups selected"));
    }
    // The selection dialog lists the groups by name.
    let selected: Vec<ModGroupRef> = all.into_iter().filter(|group| selected.contains(group)).collect();
    let mut files = Vec::new();
    for &group in &selected {
        let file_name = mod_groups.files[group.0].file_name.clone();
        if !request.dry_run {
            // `TMemIniFile.EraseSection` and `UpdateFile`, the file read
            // again for each group.
            let mut ini = MemIniFile::open(&file_name).map_err(|error| io_error(error, &file_name))?;
            ini.erase_section(&mod_groups.group(group).name);
            ini.update_file().map_err(|error| io_error(error, &file_name))?;
        }
        let shown = file_name.display().to_string();
        if !files.contains(&shown) {
            files.push(shown);
        }
    }
    Ok(ChangeResponse {
        files,
        message: None,
        dry_run: request.dry_run,
        reload: Some(reload(session, None, request.dry_run)?),
    })
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCrcsRequest {
    /// Add the current CRC32 to items that have no CRC32s ("Do you wish to
    /// add CRCs to ModGroup Items which currently do not contain any
    /// CRCs?"). As in xEdit, when some items need either and this is false
    /// the other question is not asked: the items that lack the current
    /// CRC32 are updated whatever `update` says.
    #[serde(default = "default_true")]
    pub add: bool,
    /// Add the current CRC32 to items whose CRC32s lack it ("Do you wish to
    /// update CRCs in ModGroup Items which already contain CRCs, but are
    /// missing the current one?").
    #[serde(default = "default_true")]
    pub update: bool,
    /// The modules whose items decide which mod groups need the update;
    /// every module that lacks a CRC32 when empty.
    #[serde(default)]
    pub modules: Vec<String>,
    /// The mod groups to update among those that need it; all when empty.
    /// Every item of an updated group gets its module's CRC32, as in xEdit.
    #[serde(default)]
    pub mod_groups: Vec<String>,
    /// Report what would change, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

fn modgroups_update_crcs(session: &mut Session, request: UpdateCrcsRequest) -> Result<ChangeResponse, CommandError> {
    let mut mod_groups = load_mod_groups(session)?;
    let all = mod_groups.by_name(false);
    let (missing_any, missing_current) = mod_groups.files_missing_crc(&all);
    // `AllModules := wbModulesByLoadOrder.FilteredByFlag(mfValid)`.
    let valid = |list: Vec<usize>| -> Vec<usize> {
        list.into_iter()
            .filter(|&index| mod_groups.modules.modules[index].valid)
            .collect()
    };
    let mut missing_any = valid(missing_any);
    let mut missing_current = valid(missing_current);
    if !missing_any.is_empty() && !missing_current.is_empty() {
        if !request.add {
            missing_any.clear();
        } else if !request.update {
            missing_current.clear();
        }
    }
    let nothing = |message: &str| ChangeResponse {
        files: Vec::new(),
        message: Some(message.to_owned()),
        dry_run: request.dry_run,
        reload: None,
    };
    let candidates: Vec<usize> = match (missing_any.is_empty(), missing_current.is_empty()) {
        (true, true) => return Ok(nothing("No ModGroups need updating.")),
        (false, false) => (0..mod_groups.modules.modules.len())
            .filter(|index| missing_any.contains(index) || missing_current.contains(index))
            .collect(),
        (false, true) => missing_any.clone(),
        (true, false) => missing_current.clone(),
    };
    let tagged_modules: Vec<usize> = if request.modules.is_empty() {
        candidates
    } else {
        request
            .modules
            .iter()
            .map(|name| {
                candidates
                    .iter()
                    .copied()
                    .find(|&index| same_text(&mod_groups.modules.modules[index].name, name))
                    .ok_or_else(|| {
                        CommandError::new(
                            "invalid_params",
                            format!("{name} is not a module that lacks a CRC32 in a mod group"),
                        )
                    })
            })
            .collect::<Result<_, _>>()?
    };
    let (add, update) = (!missing_any.is_empty(), !missing_current.is_empty());
    let needing = mod_groups.needing_crc_update(&all, &tagged_modules, add, update);
    if needing.is_empty() {
        return Ok(nothing("Nothing to do."));
    }
    let selected: Vec<ModGroupRef> = if request.mod_groups.is_empty() {
        needing
    } else {
        for name in &request.mod_groups {
            if !needing
                .iter()
                .any(|&group| same_text(&mod_groups.group(group).name, name))
            {
                return Err(CommandError::new(
                    "unknown_mod_group",
                    format!("no mod group named \"{name}\" needs a CRC32 update"),
                ));
            }
        }
        tagged(&mod_groups, &needing, &request.mod_groups)
    };
    let mut updated = 0;
    let mut files = Vec::new();
    for group in selected {
        if mod_groups.update_crc(group, add, update) {
            let file_name = mod_groups.files[group.0].file_name.clone();
            if !request.dry_run {
                mod_groups
                    .save_to_file(group)
                    .map_err(|error| io_error(error, &file_name))?;
            }
            updated += 1;
            let shown = file_name.display().to_string();
            if !files.contains(&shown) {
                files.push(shown);
            }
        }
    }
    let message = match updated {
        0 => "No ModGroups have been updated.".to_owned(),
        1 => "One ModGroup has been updated.".to_owned(),
        count => format!("{count} ModGroups have been updated."),
    };
    Ok(ChangeResponse {
        files,
        message: Some(message),
        dry_run: request.dry_run,
        reload: Some(reload(session, None, request.dry_run)?),
    })
}

pub fn register(registry: &mut Registry) {
    registry.register(
        "modgroups.list",
        "List the mod groups of the loaded plugins with their items, validity and selection (wbModGroupsByName).",
        false,
        modgroups_list,
    );
    registry.register(
        "modgroups.show",
        "Show one mod group with the state of each item (TwbModGroup).",
        false,
        modgroups_show,
    );
    registry.register(
        "modgroups.select",
        "Choose the active mod groups and save the selection in xEdit's settings file (mniViewModGroupsReloadClick, SaveModGroupsSelection).",
        true,
        modgroups_select,
    );
    registry.register(
        "modgroups.create",
        "Create a mod group in the .modgroups file of one of its modules (mniNavCreateModGroupClick).",
        true,
        modgroups_create,
    );
    registry.register(
        "modgroups.edit",
        "Rename a mod group or replace its items (mniNavEditModGroupClick).",
        true,
        modgroups_edit,
    );
    registry.register(
        "modgroups.delete",
        "Delete mod groups from their .modgroups files (mniNavDeleteModGroupsClick).",
        true,
        modgroups_delete,
    );
    registry.register(
        "modgroups.update_crcs",
        "Add the current CRC32s of the modules to the items of mod groups (mniNavUpdateCRCModGroupsClick).",
        true,
        modgroups_update_crcs,
    );
}
