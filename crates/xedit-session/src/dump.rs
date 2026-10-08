// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xDump.dpr

//! The `dump` command: the element tree of a plugin as `xDump.exe` prints it
//! in its plain mode (no report, no sizes, no hidden elements), with the
//! summaries of the elements without a value. The lines end with CRLF and
//! the text is in the Windows-1252 code page, as the oracle writes them.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, LazyLock};

use rayon::prelude::*;

use xedit_core::container_handler::{add_archive, add_folder, clear_containers};
use xedit_core::delphi::{change_file_ext, extract_file_path};
use xedit_core::implementation::{
    ElementImpl, FileBytes, FileImpl, MainRecordImpl, game_master_file, pin_record, trim_initialized_records, wb_file,
    wb_file_compare,
};
use xedit_core::interface::globals::{
    GameMode, ToolSource, clear_resources_loaded_handlers, game_exe_name, game_master_esm, game_name, language,
    set_create_contained_in, set_data_path, set_game_exe_name, set_game_master_esm, set_game_mode, set_game_name,
    set_hide_unused, set_language, set_simple_records, set_tool_source, wb_resources_loaded,
};
use xedit_core::interface::misc::{progress, set_progress_callback};
use xedit_core::interface::{
    Container, Element, ElementRef, File, FileState, FileStates, clear_record_defs, init_records,
};
use xedit_core::localization::{
    add_default_l_encodings_if_missing, add_l_encoding_if_missing, install_localization_handler, set_l_encoding_default,
};
use xedit_core::threads::{self, init_cycles};
use xedit_io::Encoding;

/// Sends the progress messages to stderr, like the log of xDump.
pub fn log_progress_to_stderr() {
    set_progress_callback(Some(Arc::new(|status: &str| eprintln!("{status}"))));
}

/// The game modes `xDump.exe` can dump plugins of, with their switch: the
/// name of the game mode without `gm`. `gmTES4R` has no case in xDump.
const GAMES: [(&str, GameMode); 13] = [
    ("tes3", GameMode::gmTES3),
    ("tes4", GameMode::gmTES4),
    ("fo3", GameMode::gmFO3),
    ("fnv", GameMode::gmFNV),
    ("tes5", GameMode::gmTES5),
    ("enderal", GameMode::gmEnderal),
    ("fo4", GameMode::gmFO4),
    ("sse", GameMode::gmSSE),
    ("tes5vr", GameMode::gmTES5VR),
    ("enderalse", GameMode::gmEnderalSE),
    ("fo4vr", GameMode::gmFO4VR),
    ("fo76", GameMode::gmFO76),
    ("sf1", GameMode::gmSF1),
];

/// The tag of a game mode as `--game` takes it.
pub fn game_tag(mode: GameMode) -> &'static str {
    GAMES
        .iter()
        .find(|(_, game)| *game == mode)
        .map_or("unknown", |(tag, _)| tag)
}

/// Port of the game setup of `xDump.dpr` for the plugins of a game: the
/// names of the game, its executable and its master, and the definitions.
pub fn setup_game(game: &str) -> Result<GameMode, String> {
    setup(game, None, false)
}

/// The game setup of the editor (`xeInit.pas`) for a session: as
/// [`setup_game`], with the settings the GUI reads before it builds the
/// definitions and that xDump changes: simple records (`wbSimpleRecords`,
/// the GUI's default), the unused fields hidden (`wbHideUnused`) and the
/// contained-in elements of the records in a worldspace, cell or topic
/// group (`wbCreateContainedIn`, off for Morrowind only).
pub fn setup_game_for_edit(game: &str) -> Result<GameMode, String> {
    setup(game, None, true)
}

/// Port of the game setup of `xDump.dpr -saves` for a save or co-save of a
/// game: the save definitions, switched to the co-save ones for the
/// extension of the script extender.
pub fn setup_saves(game: &str, path: &str) -> Result<GameMode, String> {
    setup(game, Some(path), false)
}

/// The save definitions of a game.
struct SaveDefinitions {
    /// The definitions of the saves.
    define: fn(),
    /// The switch to the definitions of the co-saves.
    switch_to_co_save: fn(),
    /// The extension of the co-saves of the script extender.
    co_save_extension: &'static str,
}

/// The save definitions of a game, `None` for a game without saves.
fn save_definitions(mode: GameMode) -> Option<SaveDefinitions> {
    use xedit_defs::callbacks::fnvsaves::{define_fnv_saves, switch_to_fnv_co_save};
    use xedit_defs::callbacks::fo3saves::{define_fo3_saves, switch_to_fo3_co_save};
    use xedit_defs::callbacks::fo4saves::{define_fo4_saves, switch_to_fo4_co_save};
    use xedit_defs::callbacks::tes4saves::{define_tes4_saves, switch_to_tes4_co_save};
    use xedit_defs::callbacks::tes5saves::{define_tes5_saves, switch_to_tes5_co_save};
    match mode {
        GameMode::gmTES4 => Some(SaveDefinitions {
            define: define_tes4_saves,
            switch_to_co_save: switch_to_tes4_co_save,
            co_save_extension: "obse",
        }),
        GameMode::gmFO3 => Some(SaveDefinitions {
            define: define_fo3_saves,
            switch_to_co_save: switch_to_fo3_co_save,
            co_save_extension: "fose",
        }),
        GameMode::gmFNV => Some(SaveDefinitions {
            define: define_fnv_saves,
            switch_to_co_save: switch_to_fnv_co_save,
            co_save_extension: "nvse",
        }),
        GameMode::gmFO4 => Some(SaveDefinitions {
            define: define_fo4_saves,
            switch_to_co_save: switch_to_fo4_co_save,
            co_save_extension: "f4se",
        }),
        GameMode::gmTES5 | GameMode::gmEnderal | GameMode::gmSSE | GameMode::gmEnderalSE => Some(SaveDefinitions {
            define: define_tes5_saves,
            switch_to_co_save: switch_to_tes5_co_save,
            co_save_extension: "skse",
        }),
        _ => None,
    }
}

fn setup(game: &str, save: Option<&str>, edit: bool) -> Result<GameMode, String> {
    // `xDump.dpr`: `wbAllowInternalEdit := False`, so a dump shows a record
    // as loaded, without the required subrecords the editor would add.
    xedit_core::interface::globals::set_allow_internal_edit(false);
    xedit_core::interface::globals::set_sort_sub_records(false);
    xedit_core::interface::globals::set_display_load_order_form_id(false);
    let tag = game.to_ascii_lowercase();
    let Some(&(_, mode)) = GAMES.iter().find(|(name, _)| *name == tag) else {
        let tags: Vec<&str> = GAMES.iter().map(|(tag, _)| *tag).collect();
        return Err(format!("unknown game {game}: use one of {}", tags.join(", ")));
    };
    // `wbGameName`, `wbGameExeName` without `.exe` when it is not the game
    // name, and `wbGameMasterEsm` when it is not the game name plus `.esm`.
    let (game_name, exe_name, master_esm): (&str, Option<&str>, Option<&str>) = match mode {
        GameMode::gmTES3 => ("Morrowind", None, None),
        GameMode::gmTES4 => ("Oblivion", None, None),
        GameMode::gmFO3 => ("Fallout3", None, None),
        GameMode::gmFNV => ("FalloutNV", None, None),
        GameMode::gmTES5 => ("Skyrim", Some("TESV"), None),
        GameMode::gmEnderal => ("Enderal", Some("TESV"), Some("Skyrim.esm")),
        GameMode::gmTES5VR => ("Skyrim", Some("SkyrimVR"), None),
        GameMode::gmFO4 => ("Fallout4", None, None),
        GameMode::gmFO4VR => ("Fallout4", Some("Fallout4VR"), None),
        GameMode::gmSSE => ("Skyrim", Some("SkyrimSE"), None),
        GameMode::gmEnderalSE => ("Enderal", Some("SkyrimSE"), Some("Skyrim.esm")),
        GameMode::gmFO76 => ("Fallout76", None, Some("SeventySix.esm")),
        GameMode::gmSF1 => ("Starfield", None, None),
        GameMode::gmTES4R => unreachable!("not in GAMES"),
    };
    let define: fn() = match mode {
        GameMode::gmTES5 | GameMode::gmEnderal | GameMode::gmTES5VR | GameMode::gmSSE | GameMode::gmEnderalSE => {
            xedit_defs::tes5::define_tes5
        }
        GameMode::gmFO4 | GameMode::gmFO4VR => xedit_defs::fo4::define_fo4,
        GameMode::gmTES3 => xedit_defs::tes3::define_tes3,
        GameMode::gmTES4 => xedit_defs::tes4::define_tes4,
        GameMode::gmFO3 => xedit_defs::fo3::define_fo3,
        GameMode::gmFNV => xedit_defs::fnv::define_fnv,
        GameMode::gmFO76 => xedit_defs::fo76::define_fo76,
        GameMode::gmSF1 => xedit_defs::sf1::define_sf1,
        _ => return Err(format!("the definitions of {tag} are not ported yet")),
    };
    set_simple_records(edit);
    set_hide_unused(edit);
    set_game_mode(mode);
    // xDump turns the contained-in elements off for Fallout 4 and later, the
    // editor for Morrowind.
    set_create_contained_in(if edit {
        mode != GameMode::gmTES3
    } else {
        !matches!(
            mode,
            GameMode::gmFO4 | GameMode::gmFO4VR | GameMode::gmFO76 | GameMode::gmSF1
        )
    });
    set_game_name(game_name);
    set_game_exe_name(&format!("{}.exe", exe_name.unwrap_or(game_name)));
    set_game_master_esm(&master_esm.map_or_else(|| format!("{game_name}.esm"), str::to_owned));
    clear_record_defs();
    clear_resources_loaded_handlers();
    match save {
        None => {
            set_tool_source(ToolSource::tsPlugins);
            define();
        }
        Some(path) => {
            set_tool_source(ToolSource::tsSaves);
            let saves =
                save_definitions(mode).ok_or_else(|| format!("the save definitions of {tag} are not ported yet"))?;
            let is_co_save = Path::new(path)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case(saves.co_save_extension));
            // The oracle warns that the saves of Fallout 3 and Oblivion are
            // not supported yet, and reads them all the same.
            if !is_co_save && matches!(mode, GameMode::gmFO3 | GameMode::gmTES4) {
                eprintln!("Save are not supported yet \"{path}\". Please check the command line parameters.");
            }
            (saves.define)();
            if is_co_save {
                (saves.switch_to_co_save)();
            }
        }
    }
    init_records();
    setup_language(mode);
    Ok(mode)
}

/// Whether the archives of the game are BA2 files (`wbArchiveExtension`).
fn uses_ba2(mode: GameMode) -> bool {
    matches!(
        mode,
        GameMode::gmFO4 | GameMode::gmFO4VR | GameMode::gmFO76 | GameMode::gmSF1
    )
}

/// Port of the language setup of `xDump.dpr`: the default language of the
/// game and the encodings of its string tables. The language in the game's
/// INI files is not read.
fn setup_language(mode: GameMode) {
    set_language(if uses_ba2(mode) { "En" } else { "English" });
    if mode <= GameMode::gmEnderal {
        add_default_l_encodings_if_missing(false);
    } else {
        set_l_encoding_default(Encoding::Utf8, false);
        match mode {
            GameMode::gmSSE | GameMode::gmTES5VR | GameMode::gmEnderalSE => {
                add_l_encoding_if_missing("english", Encoding::Mbcs(1252), false)
            }
            _ => add_l_encoding_if_missing("en", Encoding::Mbcs(1252), false),
        }
    }
    add_default_l_encodings_if_missing(true);
}

/// Port of the resource loading of `xDump.dpr`: when the plugin or one of
/// its masters is localized and a loose `.STRINGS` file is missing, the
/// archives of each master and of the plugin (`<name>`, `<name> - Interface`,
/// `<name> - Localization`, `<name> - Wwise*`) are added, then the data folder.
pub(crate) fn load_resources(file: &FileImpl, path: &str, data_path: &str, mode: GameMode) {
    let data_path = data_path.to_owned();
    set_data_path(&data_path);
    // The loose strings files are looked for next to the file, which for a
    // save is not the data folder.
    let file_path = extract_file_path(path);
    clear_containers();
    let mut names: Vec<String> = file.masters().iter().map(|master| master.get_name()).collect();
    names.push(file.get_name());
    let is_localized = file.get_is_localized() || file.masters().iter().any(|master| master.get_is_localized());
    let load_archives = is_localized
        && names.iter().any(|name| {
            let strings = format!(
                "{file_path}Strings\\{}_{}.STRINGS",
                change_file_ext(name, ""),
                language()
            );
            !Path::new(&strings).is_file()
        });
    if load_archives {
        let extension = if uses_ba2(mode) { ".ba2" } else { ".bsa" };
        for name in &names {
            let stem = change_file_ext(name, "");
            for suffix in ["", " - Interface", " - Localization"] {
                let archive = format!("{data_path}{stem}{suffix}{extension}");
                if Path::new(&archive).is_file() {
                    add_resource_archive(&archive);
                }
            }
            // The Wwise archives match by prefix.
            let prefix = format!("{stem} - Wwise").to_ascii_lowercase();
            let mut wwise: Vec<String> = std::fs::read_dir(&data_path)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|file_name| {
                    let lower = file_name.to_ascii_lowercase();
                    lower.starts_with(&prefix) && lower.ends_with(extension)
                })
                .collect();
            wwise.sort();
            for file_name in wwise {
                add_resource_archive(&format!("{data_path}{file_name}"));
            }
        }
    }
    add_folder(Path::new(&data_path));
    wb_resources_loaded();
    install_localization_handler();
}

fn add_resource_archive(archive: &str) {
    progress(&format!(
        "[{}] Loading Resources.",
        xedit_core::delphi::path_file_name(archive)
    ));
    if let Err(error) = add_archive(Path::new(archive)) {
        progress(&error.to_string());
    }
}

/// Loads the plugin with its masters, its resources and the hardcoded
/// records, as `xDump.dpr` does before it starts the dump.
pub fn load_file(path: &str, mode: GameMode) -> Result<Arc<FileImpl>, String> {
    let file = wb_file(path, i32::MAX, FileStates::empty()).map_err(|error| error.to_string())?;
    load_resources(&file, path, &extract_file_path(path), mode);
    load_hardcoded()?;
    Ok(file)
}

/// Loads a save or co-save with the plugins it lists from `data_path`, the
/// resources and the hardcoded records, and writes its dump.
pub fn dump_save(path: &str, data_path: &str, mode: GameMode, out: &mut dyn Write) -> Result<(), String> {
    let mut data_path = data_path.to_owned();
    if !data_path.ends_with(['\\', '/']) {
        data_path.push('\\');
    }
    set_data_path(&data_path);
    let file = wb_file(path, i32::MAX, FileStates::empty()).map_err(|error| error.to_string())?;
    load_resources(&file, path, &data_path, mode);
    load_hardcoded()?;
    // A save has no main records to spread over the workers, and the save
    // definitions keep the tables of the save being read in globals.
    write_elements(&*file, 0, None, out).map_err(|error| error.to_string())
}

/// Loads the plugin and writes its dump.
pub fn dump_file(path: &str, mode: GameMode, out: &mut dyn Write) -> Result<(), String> {
    let file = load_file(path, mode)?;
    write_container(&file, out).map_err(|error| error.to_string())
}

/// Writes the dump of a loaded file, on the threads of `threads::threads`.
pub fn write_dump(file: &FileImpl, out: &mut dyn Write) -> std::io::Result<()> {
    write_container(file, out)
}

/// Port of the hardcoded load of `xDump.dpr`: when the game master is
/// loaded, the embedded plugin of the hardcoded records loads in its place
/// under the name of the game executable.
pub(crate) fn load_hardcoded() -> Result<(), String> {
    if game_master_file().is_none() {
        return Ok(());
    }
    let Some(bytes) = xedit_defs::hardcoded::hardcoded_dat(&game_name()) else {
        return Ok(());
    };
    let mut states = FileStates::empty();
    states.include(FileState::fsIsHardcoded);
    wb_file_compare(
        &game_exe_name(),
        0,
        Some(&game_master_esm()),
        states,
        FileBytes::Owned(bytes.to_vec()),
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Port of `WriteContainer`. With more than one thread the main records
/// are built and written on worker threads (`ParallelDump`); the output is
/// the same.
fn write_container(container: &FileImpl, out: &mut dyn Write) -> std::io::Result<()> {
    match threads::pool() {
        None => write_elements(container, 0, None, out),
        Some(pool) => ParallelDump::new(pool, out).run(container),
    }
}

/// Writes the elements of `container` at nesting `depth`. `record` is the
/// main record the elements belong to.
fn write_elements(
    container: &dyn Container,
    depth: usize,
    record: Option<&Arc<MainRecordImpl>>,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    for index in 0..container.get_element_count() {
        if let Some(element) = container.get_element(index) {
            write_element(&element, depth, record, out)?;
        }
    }
    Ok(())
}

/// A main record of a batch, written by a worker at its place in the
/// output.
struct RecordJob {
    record: Arc<MainRecordImpl>,
    element: ElementRef,
    depth: usize,
}

/// The output of a batch in file order.
enum Segment {
    /// The line of an element outside of the main records (the file, a
    /// group), with the element and its depth to write it again.
    Line(ElementRef, usize, Vec<u8>),
    /// `<contents skipped>` at a depth.
    Skipped(usize),
    /// A main record, by its index in the batch.
    Record(usize),
}

/// A batch ends after this many main records, or after records with this
/// much data, whichever comes first.
const BATCH_RECORDS: usize = 512;
const BATCH_BYTES: usize = 4 << 20;

/// The batches on the workers at most. A record that takes long holds the
/// output of the batches after it, which bounds the memory.
const BATCHES_IN_FLIGHT: usize = 4;

/// A batch whose records are written on the workers.
struct Batch {
    segments: Vec<Segment>,
    jobs: Arc<Vec<RecordJob>>,
    /// `init_cycles` when the walk of the batch began.
    cycles: u64,
    written: std::sync::mpsc::Receiver<std::io::Result<Vec<Vec<u8>>>>,
}

/// The dump with the main records built and written on worker threads.
///
/// The calling thread walks the tree and writes the lines of the file and
/// its groups; the main records are collected in batches, and the workers
/// build and write each record of a batch into a buffer of its own while the
/// walk goes on. The batches are written to the output in file order, so
/// the output is the serial output. A worker writes a record as the serial
/// dump does, trimming the other records after each line and resetting the
/// record at the end; a record that another thread reads at that moment is
/// built again for it (see `xedit_core::threads`), and the records the
/// workers write are pinned, so that a trim does not reset them.
///
/// When two builds needed each other (`init_cycles`), what the threads built
/// meanwhile could depend on timing: the dump waits for the workers, resets
/// the records, and writes every batch not yet written again on the calling
/// thread alone.
struct ParallelDump<'a> {
    pool: &'static rayon::ThreadPool,
    out: &'a mut dyn Write,
    /// The batch being collected.
    segments: Vec<Segment>,
    jobs: Vec<RecordJob>,
    batch_bytes: usize,
    batch_cycles: u64,
    /// The batches on the workers, in file order.
    in_flight: std::collections::VecDeque<Batch>,
}

impl<'a> ParallelDump<'a> {
    fn new(pool: &'static rayon::ThreadPool, out: &'a mut dyn Write) -> Self {
        ParallelDump {
            pool,
            out,
            segments: Vec::new(),
            jobs: Vec::new(),
            batch_bytes: 0,
            batch_cycles: init_cycles(),
            in_flight: std::collections::VecDeque::new(),
        }
    }

    fn run(mut self, container: &FileImpl) -> std::io::Result<()> {
        self.walk_elements(container, 0)?;
        self.flush()?;
        while !self.in_flight.is_empty() {
            self.write_first()?;
        }
        Ok(())
    }

    fn walk_elements(&mut self, container: &dyn Container, depth: usize) -> std::io::Result<()> {
        for index in 0..container.get_element_count() {
            if let Some(element) = container.get_element(index) {
                self.walk_element(&element, depth)?;
            }
        }
        Ok(())
    }

    /// `write_element` for the elements outside of the main records; a main
    /// record goes into the batch.
    fn walk_element(&mut self, element: &ElementRef, depth: usize) -> std::io::Result<()> {
        if let Some(record) = element.as_element_impl().and_then(ElementImpl::main_record_impl) {
            self.batch_bytes += record.header_struct().data_size as usize;
            self.segments.push(Segment::Record(self.jobs.len()));
            self.jobs.push(RecordJob {
                record,
                element: element.clone(),
                depth,
            });
            if self.jobs.len() >= BATCH_RECORDS || self.batch_bytes >= BATCH_BYTES {
                self.flush()?;
            }
            return Ok(());
        }
        if trace_enabled() {
            eprintln!("{:width$}{}", "", element.get_name(), width = depth * 2);
        }
        let mut line = Vec::new();
        let (name, child_depth) = {
            let _read = threads::read_guard();
            let name = element.get_display_name(true);
            let child_depth = write_line(element, &name, depth, &mut line)?;
            (name, child_depth)
        };
        self.segments.push(Segment::Line(element.clone(), depth, line));
        if let Some(container) = element.as_container()
            && !name.starts_with("Hidden: ")
        {
            if element.get_skipped() {
                self.segments.push(Segment::Skipped(child_depth));
            } else {
                self.walk_elements(container, child_depth)?;
            }
        }
        Ok(())
    }

    /// Hands the collected batch to the workers, and writes the batches
    /// before it that are done while more than `BATCHES_IN_FLIGHT` run.
    fn flush(&mut self) -> std::io::Result<()> {
        let jobs = Arc::new(std::mem::take(&mut self.jobs));
        let (sender, written) = std::sync::mpsc::channel();
        let batch_jobs = jobs.clone();
        self.pool.spawn(move || {
            let result = batch_jobs
                .par_iter()
                .with_max_len(1)
                .map(|job| {
                    // The record stays built while the worker writes it, and
                    // is reset at the end as on one thread.
                    let _pin = pin_record(&job.record);
                    let mut buffer = Vec::new();
                    write_element(&job.element, job.depth, None, &mut buffer)?;
                    Ok(buffer)
                })
                .collect();
            // The dump may have stopped at an error and dropped the receiver.
            let _ = sender.send(result);
        });
        self.in_flight.push_back(Batch {
            segments: std::mem::take(&mut self.segments),
            jobs,
            cycles: self.batch_cycles,
            written,
        });
        self.batch_bytes = 0;
        self.batch_cycles = init_cycles();
        while self.in_flight.len() > BATCHES_IN_FLIGHT {
            self.write_first()?;
        }
        Ok(())
    }

    /// Waits for the first batch on the workers and writes it.
    fn write_first(&mut self) -> std::io::Result<()> {
        let Some(batch) = self.in_flight.pop_front() else {
            return Ok(());
        };
        let written = batch
            .written
            .recv()
            .unwrap_or_else(|_| Err(std::io::Error::other("a dump worker stopped")))?;
        if init_cycles() == batch.cycles {
            return write_batch(self.out, &batch.segments, &written);
        }
        // Two builds needed each other since this batch began: no thread
        // may build anything while the batches not yet written are written
        // again, from records that are not built.
        let mut batches = vec![batch];
        while let Some(batch) = self.in_flight.pop_front() {
            let _ = batch.written.recv();
            batches.push(batch);
        }
        eprintln!(
            "Warning: two records needed each other while they were built; writing the records again on one thread"
        );
        for batch in &batches {
            for job in batch.jobs.iter() {
                job.record.reset();
            }
        }
        trim_initialized_records(0, None);
        for batch in &batches {
            let written = batch
                .jobs
                .iter()
                .map(|job| {
                    let mut buffer = Vec::new();
                    write_element(&job.element, job.depth, None, &mut buffer)?;
                    Ok(buffer)
                })
                .collect::<std::io::Result<Vec<_>>>()?;
            let mut segments = Vec::with_capacity(batch.segments.len());
            for segment in &batch.segments {
                segments.push(match segment {
                    Segment::Line(element, depth, _) => {
                        let mut line = Vec::new();
                        write_line(element, &element.get_display_name(true), *depth, &mut line)?;
                        Segment::Line(element.clone(), *depth, line)
                    }
                    Segment::Skipped(depth) => Segment::Skipped(*depth),
                    Segment::Record(index) => Segment::Record(*index),
                });
            }
            write_batch(self.out, &segments, &written)?;
        }
        Ok(())
    }
}

/// Writes the output of a batch in file order.
fn write_batch(out: &mut dyn Write, segments: &[Segment], written: &[Vec<u8>]) -> std::io::Result<()> {
    for segment in segments {
        match segment {
            Segment::Line(_, _, line) => out.write_all(line)?,
            Segment::Skipped(depth) => {
                write_indent(out, *depth)?;
                out.write_all(b"<contents skipped>\r\n")?;
            }
            Segment::Record(index) => out.write_all(&written[*index])?,
        }
    }
    Ok(())
}

/// Writes the text as xDump does: through the ANSI code page of the
/// console output (Windows-1252), where a character without a mapping
/// becomes `?`.
fn write_text(out: &mut dyn Write, text: &str) -> std::io::Result<()> {
    if text.is_ascii() {
        out.write_all(text.as_bytes())
    } else {
        out.write_all(&Encoding::Mbcs(1252).get_bytes(text))
    }
}

/// Writes the two spaces of indentation per nesting level.
fn write_indent(out: &mut dyn Write, depth: usize) -> std::io::Result<()> {
    const SPACES: &[u8; 64] = &[b' '; 64];
    let mut remaining = depth * 2;
    while remaining > 0 {
        let count = remaining.min(SPACES.len());
        out.write_all(&SPACES[..count])?;
        remaining -= count;
    }
    Ok(())
}

/// Whether `XEDIT_TRACE` is set: every element name goes to stderr.
fn trace_enabled() -> bool {
    static TRACE: LazyLock<bool> = LazyLock::new(|| std::env::var_os("XEDIT_TRACE").is_some());
    *TRACE
}

/// Port of `WriteElement` in the plain dump mode. The name, the value and
/// the summary are written straight into `out`; the summary is only
/// computed when the line needs it.
fn write_element(
    element: &ElementRef,
    depth: usize,
    record: Option<&Arc<MainRecordImpl>>,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let own_record = element.as_element_impl().and_then(ElementImpl::main_record_impl);
    let record = own_record.as_ref().or(record);
    if trace_enabled() {
        eprintln!("{:width$}{}", "", element.get_name(), width = depth * 2);
    }
    // The line reads other records, which another thread may reset.
    let (name, child_depth) = {
        let _read = threads::read_guard();
        let name = element.get_display_name(true);
        let child_depth = write_line(element, &name, depth, out)?;
        (name, child_depth)
    };
    // The other records the callbacks of this line built, such as the
    // navmeshes the edges of a navmesh link to, are released by Delphi when
    // the callback lets go of them. The port resets them once more than
    // `KEPT_RECORDS` are built; the record being written stays.
    trim_initialized_records(kept_records(), record);
    if let Some(container) = element.as_container()
        && !name.starts_with("Hidden: ")
    {
        if element.get_skipped() {
            write_indent(out, child_depth)?;
            out.write_all(b"<contents skipped>\r\n")?;
        } else {
            write_elements(container, child_depth, record, out)?;
        }
    }
    // `WriteContainer` holds an `IwbContainerElementRef` on the record while
    // it writes the elements; releasing it resets the record and frees the
    // subrecords, so the dump never holds more than one record tree.
    if let Some(own_record) = &own_record {
        own_record.reset();
    }
    Ok(())
}

/// The line `WriteElement` writes for the element with the display name
/// `name`: the name with the value or the summary. Returns the depth of the
/// children.
fn write_line(element: &ElementRef, name: &str, depth: usize, out: &mut dyn Write) -> std::io::Result<usize> {
    let value = element.get_value();
    let mut child_depth = depth;
    if element.get_name() != "Unused" && name != "Unused" {
        if !name.is_empty() {
            write_indent(out, depth)?;
            write_text(out, name)?;
        }
        if !name.is_empty() || !value.is_empty() {
            child_depth += 1;
        }
        if !name.starts_with("Hidden: ") {
            if !value.is_empty() {
                out.write_all(b": ")?;
                write_text(out, &value)?;
                out.write_all(b"\r\n")?;
            } else if !name.is_empty() {
                let summary = element.get_summary();
                if summary.is_empty() {
                    out.write_all(b"\r\n")?;
                } else {
                    out.write_all(b" [S]: ")?;
                    write_text(out, &summary)?;
                    out.write_all(b"\r\n")?;
                }
            }
        }
    }
    Ok(child_depth)
}

/// The number of other records whose subrecords stay built while the dump
/// goes on, so that the records the callbacks read often are not built
/// again for every line.
///
/// Measured on `DLCCoast.esm`: 64 records peak at 1.3 GB in 78 s, 256 at
/// 1.6 GB in 69 s, 1024 at 2.1 GB in 67 s.
const KEPT_RECORDS: usize = 256;

/// The kept records of `KEPT_RECORDS`, times this for the workers of the
/// parallel dump, which share them.
const KEPT_RECORDS_PER_WORKER: usize = 4;

fn kept_records() -> usize {
    match rayon::current_thread_index() {
        Some(_) => KEPT_RECORDS * KEPT_RECORDS_PER_WORKER,
        None => KEPT_RECORDS,
    }
}
