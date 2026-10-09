// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

mod engine;
mod mcp;
mod pipe;
mod rpc;
mod serve;

use std::io::BufReader;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use xedit_session::{CommandError, Registry, parse_batch};

/// The dump allocates and frees millions of small strings and element
/// nodes; mimalloc serves them faster than the Windows heap.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// xEdit command-line interface.
#[derive(Parser)]
#[command(name = "xedit", version)]
struct Cli {
    /// Print one JSON envelope: {"ok":true,"result":...} or {"ok":false,"error":{"code","message"}}.
    #[arg(long, global = true)]
    json: bool,

    /// Game of the plugins to load, as the xDump switch: tes3, tes4, fo3, fnv, tes5, enderal, fo4, sse, tes5vr, enderalse, fo4vr, fo76 or sf1.
    #[arg(long, global = true)]
    game: Option<String>,

    /// Plugin to load, in load order; repeat for several. Masters load with it.
    #[arg(long, global = true)]
    load: Vec<String>,

    /// Allow the commands that change plugin data or files to do so. Without it they only run with --dry-run.
    #[arg(long, global = true)]
    edit: bool,

    /// Give the topic responses without PNAM the PNAM of the response they follow when they are built (xEdit's -FillPNAM; the quick clean mode turns it on).
    #[arg(long, global = true)]
    fill_pnam: bool,

    /// Threads that load the plugins and build the records of a dump; 1 runs everything on one thread. Default: RAYON_NUM_THREADS, else one per CPU. The output is the same for every count.
    #[arg(long, global = true)]
    threads: Option<usize>,

    /// Folder of the reference cache files (xEdit's -C:). Default: "<AppName>Edit Cache" in the data folder, as xEdit.
    #[arg(long, global = true)]
    cache_path: Option<String>,

    /// Neither read nor write reference cache files (xEdit's -DontCache).
    #[arg(long, global = true)]
    dont_cache: bool,

    /// Do not read reference cache files (xEdit's -DontCacheLoad).
    #[arg(long, global = true)]
    dont_cache_load: bool,

    /// Do not write reference cache files (xEdit's -DontCacheSave).
    #[arg(long, global = true)]
    dont_cache_save: bool,

    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Print every command with its request and response JSON Schema.
    Schema,
    /// The game and the loaded plugins.
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// The loaded files.
    Files {
        #[command(subcommand)]
        action: FilesAction,
    },
    /// Records of the loaded plugins.
    Records {
        #[command(subcommand)]
        action: RecordsAction,
    },
    /// Elements of a record.
    Elements {
        #[command(subcommand)]
        action: ElementsAction,
    },
    /// Masters of a plugin. Each command rewrites the FormIDs of the plugin to follow its masters; save to keep the change.
    Masters {
        #[command(subcommand)]
        action: MastersAction,
    },
    /// The reference index: the records that refer to a record, and the reference cache.
    Refs {
        #[command(subcommand)]
        action: RefsAction,
    },
    /// FormIDs of records.
    Formids {
        #[command(subcommand)]
        action: FormidsAction,
    },
    /// BSA and BA2 archives: list, extract and pack, the modes of BSArch. They need no --game or --load.
    Archive {
        #[command(subcommand)]
        action: ArchiveAction,
    },
    /// Save files.
    Saves {
        #[command(subcommand)]
        action: SavesAction,
    },
    /// Clean a plugin (files.clean): remove the records identical to their master (--itm), undelete and disable the deleted references (--udr), or run xEdit's quick auto clean mode (--quick: both, saved, repeated while a pass changes the plugin). Without --edit only --dry-run, which counts.
    Clean {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Remove the records that are identical to their master and the groups left empty.
        #[arg(long)]
        itm: bool,
        /// Undelete and disable the deleted references.
        #[arg(long)]
        udr: bool,
        /// The quick auto clean mode of -quickautoclean; loads the plugins as that mode does (full record definitions, PNAM fill) and saves the plugin.
        #[arg(long)]
        quick: bool,
        /// Count what would be cleaned, change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Where --quick saves the plugin; the loaded path when omitted.
        #[arg(long)]
        output: Option<String>,
        /// Do not move an existing file at the output path to the backup folder.
        #[arg(long)]
        no_backup: bool,
    },
    /// Write a loaded plugin to disk as xEdit saves it (files.save).
    Save {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Path to write to; the loaded path when omitted.
        #[arg(long)]
        output: Option<String>,
        /// Build the file and report its size and CRC32, but write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Do not move an existing file at the output path to the backup folder.
        #[arg(long)]
        no_backup: bool,
    },
    /// List the conflict status of the records of the loaded plugins and of each file (conflicts.list, ConflictLevelForMainRecord). Lists the records that are not the only record of their FormID unless --include-single.
    Conflicts {
        /// List only the records of this loaded file; repeat for several. Every record is still compared with all loaded files.
        #[arg(long)]
        file: Vec<String>,
        /// List only the records with this signature, such as NPC_; repeat for several.
        #[arg(long)]
        signature: Vec<String>,
        /// List only the records whose conflict is at least this ConflictAll: caOnlyOne, caNoConflict, caConflictBenign, caOverride, caConflict or caConflictCritical.
        #[arg(long)]
        min_conflict_all: Option<String>,
        /// List only the records whose own status is this ConflictThis, such as ctConflictLoses; repeat for several.
        #[arg(long)]
        conflict_this: Vec<String>,
        /// Also list the records that are the only record of their FormID.
        #[arg(long)]
        include_single: bool,
        /// Compare only the master and the leaf overrides ("Only show Master and Leafs").
        #[arg(long)]
        master_and_leafs: bool,
        /// Classify a FormID with one override as an override without comparing, as -quickshowconflicts does.
        #[arg(long)]
        quick_show_conflicts: bool,
        /// Listed records to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Records to list at most.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Compare the records of a FormID side by side, row by row with their conflict status, as the view tab does (records.compare).
    Compare {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Compare only the master and the leaf overrides ("Only show Master and Leafs").
        #[arg(long)]
        master_and_leafs: bool,
        /// Hide the rows without a conflict ("Hide no conflict and empty rows").
        #[arg(long)]
        hide_no_conflict: bool,
        /// Also list the rows the view hides (ignored members, members no record has).
        #[arg(long)]
        include_hidden: bool,
    },
    /// Write the element tree of a plugin as xDump prints it.
    Dump {
        /// Game of the plugin, as for --game.
        #[arg(long)]
        game: String,
        /// Path of the plugin.
        file: String,
    },
    /// NIF and KF meshes, BGSM and BGEM materials, LOD, FUZ and DDS files, loose or in an archive.
    Assets {
        #[command(subcommand)]
        action: AssetsAction,
    },
    /// The operations of Sniff on the NIF, KF and material files of a folder or archive.
    Sniff {
        #[command(subcommand)]
        action: SniffAction,
    },
    /// Generate the LOD of worldspaces as xEdit's LODGen mode does (lodgen.generate), with the LODGen form's options. Load every plugin of the load order with --load. Needs --edit unless --dry-run.
    Lodgen {
        /// The editor ID of a worldspace to generate LOD for; repeat for more. Without it the form's default.
        #[arg(long = "worldspace", value_name = "EDITORID")]
        worldspaces: Vec<String>,
        /// An option of the LODGen form, NAME=VALUE (AtlasWidth=4096, BuildAtlas=0, ...); repeat for more.
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
        /// The settings file of the options (<APP>LODGen.ini); read, and written back unless a dry run.
        #[arg(long)]
        settings: Option<String>,
        /// The folder for the LOD files (-O:); the data folder by default.
        #[arg(long)]
        output: Option<String>,
        /// The folder with LODGenx64.exe, Texconvx64.exe and the atlas maps (-S:).
        #[arg(long)]
        scripts: Option<String>,
        /// The temporary folder (-T:).
        #[arg(long)]
        temp: Option<String>,
        /// The data folder (-D:); the folder of the plugins by default.
        #[arg(long)]
        data: Option<String>,
        /// The game ini whose archive lists load (-I:).
        #[arg(long)]
        game_ini: Option<String>,
        /// The RandSeed of the tree rotations; from the clock by default.
        #[arg(long)]
        seed: Option<u32>,
        /// Split the trees LOD atlas of the worldspaces into billboards (the form's hidden Split Trees LOD button) instead of generating.
        #[arg(long)]
        split_trees: bool,
        /// List the worldspaces and options, but generate nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Run a command by name.
    Call {
        /// Command name as listed by `xedit schema`, for example system.version.
        name: String,
        /// Command parameters as a JSON object.
        #[arg(long, default_value = "{}")]
        params: String,
    },
    /// Run several commands in one session: a JSON array of {"command": name, "params": {...}} from a file or stdin (-).
    Batch {
        /// Path of the JSON file, or - for stdin.
        file: String,
        /// Go on after a failed command instead of stopping at it.
        #[arg(long)]
        keep_going: bool,
    },
    /// Keep the session loaded and answer JSON-RPC requests, one per line, on stdio or a named pipe. Every command is a method; see rpc.discover.
    Serve {
        /// Listen on the Windows named pipe \\.\pipe\NAME instead of stdio.
        #[arg(long)]
        pipe: Option<String>,
    },
    /// Serve the commands as Model Context Protocol tools on stdio. The plugins load when the first tool is called.
    Mcp,
}

#[derive(Subcommand)]
enum SniffAction {
    /// List the operations with their settings and defaults (sniff.list).
    List,
    /// Run an operation on a folder or archive (sniff.run), as Sniff's -OP: does. Needs --edit unless --dry-run.
    Run {
        /// The title of the operation, any case, such as "Update bounds".
        operation: String,
        /// The folder or the archive (BSA, BA2) with the files.
        input: String,
        /// The folder to write the changed files to; not needed by operations that only report.
        #[arg(long)]
        output: Option<String>,
        /// A settings ini in Sniff's form (the section is the title without spaces).
        #[arg(long)]
        settings: Option<String>,
        /// A setting of the operation's section, NAME=VALUE; repeat for more.
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
        /// Only the files whose path holds this text.
        #[arg(long)]
        path_contains: Option<String>,
        /// Leave the subfolders of an input folder.
        #[arg(long)]
        no_subdir: bool,
        /// Report a file that fails and go on.
        #[arg(long)]
        skip_on_errors: bool,
        /// Write the unchanged files too.
        #[arg(long)]
        copy_all: bool,
        /// Threads; 0 for the CPU count less one.
        #[arg(long)]
        threads: Option<i32>,
        /// Also write the messages to this file, as Sniff's -LOG: does.
        #[arg(long)]
        log: Option<String>,
        /// Process the files and report, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum AssetsAction {
    /// Print a file as text (ToText) or JSON (ToJSON, as Sniff writes it).
    Dump {
        /// The file, or the archive that holds it.
        file: String,
        /// The path of the file inside the archive FILE.
        #[arg(long)]
        archive_path: Option<String>,
        /// The format of the file: nif, bgsm, bgem, lod, dlodsettings, lst, btt, fuz or dds; by the extension when omitted.
        #[arg(long)]
        kind: Option<String>,
        /// text or json.
        #[arg(long)]
        format: Option<String>,
        /// Decimals of the float values, 6 to 16.
        #[arg(long)]
        decimals: Option<usize>,
        /// Rotations as Euler angles in degrees instead of an angle and an axis.
        #[arg(long)]
        euler: bool,
    },
    /// List the blocks of a NIF file.
    Blocks {
        /// The file, or the archive that holds it.
        file: String,
        /// The path of the file inside the archive FILE.
        #[arg(long)]
        archive_path: Option<String>,
        /// The format of the file; by the extension when omitted.
        #[arg(long)]
        kind: Option<String>,
    },
    /// List the NIF block types.
    Types,
    /// Load a file and write it back as xEdit saves it.
    Save {
        /// The file, or the archive that holds it.
        file: String,
        /// Path to write to.
        #[arg(long)]
        output: String,
        /// The path of the file inside the archive FILE.
        #[arg(long)]
        archive_path: Option<String>,
        /// The format of the file; by the extension when omitted.
        #[arg(long)]
        kind: Option<String>,
        /// Build the file and report it, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Set a value of a file and write it.
    Set {
        /// The file, or the archive that holds it.
        file: String,
        /// Path of the element, with \ between the names, below BLOCK for a NIF.
        path: String,
        /// The new value as the dump prints it.
        value: String,
        /// Path to write to.
        #[arg(long)]
        output: String,
        /// For a NIF: header, footer, a block index or a block path.
        #[arg(long)]
        block: Option<String>,
        /// The path of the file inside the archive FILE.
        #[arg(long)]
        archive_path: Option<String>,
        /// The format of the file; by the extension when omitted.
        #[arg(long)]
        kind: Option<String>,
        /// Report the value before and after, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Build a file from its JSON form and write it.
    FromJson {
        /// The JSON file.
        file: String,
        /// Path to write to.
        #[arg(long)]
        output: String,
        /// The format to build; by the extension before .json when omitted.
        #[arg(long)]
        kind: Option<String>,
        /// Build the file and report it, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum SavesAction {
    /// Write the element tree of a save or co-save as `xDump -saves` prints it.
    Dump {
        /// Game of the save, as for --game.
        #[arg(long)]
        game: String,
        /// Data folder of the game, where the plugins the save lists are.
        #[arg(long)]
        data: String,
        /// Path of the save or co-save.
        file: String,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Report the game, the data folder and the loaded plugins.
    Info,
}

#[derive(Subcommand)]
enum FilesAction {
    /// List the loaded files with their masters and record counts.
    List,
    /// Read or set the module flags of a plugin header (files.flags). Needs --edit unless --dry-run; --dry-run alone reads them.
    Flags {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// The ESM flag.
        #[arg(long)]
        esm: Option<bool>,
        /// The light (ESL) flag, where the game has it.
        #[arg(long)]
        light: Option<bool>,
        /// The medium flag (Starfield); clears light and update.
        #[arg(long)]
        medium: Option<bool>,
        /// The update (overlay) flag (Starfield); clears light and medium.
        #[arg(long, alias = "overlay")]
        update: Option<bool>,
        /// The blueprint flag (Starfield).
        #[arg(long)]
        blueprint: Option<bool>,
        /// The localized flag.
        #[arg(long)]
        localized: Option<bool>,
        /// Report the flags the change would give, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum RefsAction {
    /// The records that refer to a record (ReferencedBy of its master) and the records it refers to (refs.get). Builds the references of the loaded files first, or loads them from the cache.
    Get {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// Plugin whose version of the record to read; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Entries of the referenced-by list to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Entries of the referenced-by list to return at most.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Write the referenced-by lists of every loaded file as text (the parity check's format, see refs.pas of the harness).
    Dump,
    /// Build the references of the loaded files, or load them from the reference cache, and save the cache (refs.build, BuildOrLoadRef).
    Build {
        /// Only this loaded file (Build Reference Info); every loaded file when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Only load the references from the cache; build none.
        #[arg(long)]
        only_load: bool,
enum ArchiveAction {
    /// Read the header and the file table of an archive (archive.list): format, version, flags, warnings and with --files the files.
    List {
        /// Path of the archive.
        archive: String,
        /// List the files.
        #[arg(long)]
        files: bool,
        /// Only the files below this folder of the archive.
        #[arg(long)]
        folder: Option<String>,
        /// Files to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Files to return at most.
        #[arg(long, default_value_t = 1000)]
        limit: usize,
    },
    /// Unpack an archive into a folder (archive.extract). Needs --edit unless --dry-run.
    Extract {
        /// Path of the archive.
        archive: String,
        /// Folder that exists to unpack into; the folder of the archive when omitted.
        output: Option<String>,
        /// Threads that decompress and write; 0 uses every CPU. The files are the same for every count.
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Report what would be written, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Pack folders, files and archives into an archive (archive.pack), byte for byte as BSArch does. Needs --edit unless --dry-run.
    Pack {
        /// Path of the archive to write.
        archive: String,
        /// Folders, files and archives to pack; later ones win on files with the same name.
        #[arg(required = true)]
        sources: Vec<String>,
        /// Archive format: tes3, tes4, fo3, fnv, tes5, sse, fo4, fo4dds, sf1 or sf1dds.
        #[arg(long)]
        format: String,
        /// Compress the files: zlib, lz4, lz4f, or without a value the default of the format.
        #[arg(short = 'z', long, num_args = 0..=1, default_missing_value = "default")]
        compress: Option<String>,
        /// Split into archives of this many GB (at most 8), 0 for none. Default: BSA formats 2 GB, BA2 formats none.
        #[arg(long, allow_hyphen_values = true)]
        split: Option<i64>,
        /// Pack only the files whose name matches one of these masks (* and ?); repeat for several.
        #[arg(long = "filter")]
        filters: Vec<String>,
        /// Do not let identical files share their data.
        #[arg(long)]
        no_share: bool,
        /// Threads that read and compress; 0 uses every CPU. The archives are the same for every count.
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Override the archive flags of a BSA with this hexadecimal value.
        #[arg(long)]
        archive_flags: Option<String>,
        /// Override the file flags of a BSA with this hexadecimal value.
        #[arg(long)]
        file_flags: Option<String>,
        /// Add the sources and report the files that would be packed, but write nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum FormidsAction {
    /// Change the FormID of a record and update the records that refer to it (formids.change). Needs --edit unless --dry-run.
    Change {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// The new load order FormID; the next free FormID of the file (or of --target-file) when omitted.
        new_form_id: Option<String>,
        /// Plugin whose version of the record changes; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Take the next free FormID of this file: the record's file or one of its masters.
        #[arg(long)]
        target_file: Option<String>,
        /// Change the later overrides of the record too.
        #[arg(long)]
        overrides: bool,
        /// Report the change and the referencing records, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Renumber the new records of a plugin (formids.renumber). Needs --edit unless --dry-run.
    Renumber {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// The first object ID in hexadecimal (six digits, three for a light target); the next object ID when omitted.
        #[arg(long)]
        start: Option<String>,
        /// Compact the FormIDs into the ESL range from 000800.
        #[arg(long)]
        compact: bool,
        /// Give the records FormIDs of this master instead.
        #[arg(long)]
        inject_into: Option<String>,
        /// With --inject-into: keep the object IDs the master has free.
        #[arg(long)]
        preserve_object_ids: bool,
        /// With --preserve-object-ids: stop when an object ID can not be kept.
        #[arg(long)]
        all_or_nothing: bool,
        /// Report the plan, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum RecordsAction {
    /// List the records of a plugin.
    List {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Keep only records with this signature, such as NPC_.
        #[arg(long)]
        signature: Option<String>,
        /// Records to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Records to return at most.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Find records whose editor ID or name contains a text.
    Find {
        /// Plugin name; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Keep only records with this signature.
        #[arg(long)]
        signature: Option<String>,
        /// Text the editor ID contains, compared without case.
        #[arg(long)]
        editor_id: Option<String>,
        /// Text the name contains, compared without case.
        #[arg(long)]
        name: Option<String>,
        /// Records to return at most.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Read a record with all its elements.
    Get {
        /// Load order FormID as hexadecimal digits.
        form_id: String,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Levels of child elements to include; all when omitted.
        #[arg(long)]
        depth: Option<usize>,
    },
    /// Copy a record into a plugin as an override or a new record (records.copy, xEdit's CopyInto). Needs --edit unless --dry-run.
    Copy {
        /// Load order FormID of the record to copy as hexadecimal digits.
        form_id: String,
        /// Plugin to copy into.
        #[arg(long)]
        to: String,
        /// Plugin the record is seen from (that version is copied); the last loaded plugin when omitted.
        #[arg(long)]
        from: Option<String>,
        /// Copy as a new record with a new FormID instead of an override.
        #[arg(long)]
        as_new: bool,
        /// Copy the records of the child group too (the references of a cell, the responses of a topic).
        #[arg(long)]
        deep: bool,
        /// Text put before the editor ID of the copy.
        #[arg(long, default_value = "")]
        prefix: String,
        /// Text put after the editor ID of the copy.
        #[arg(long, default_value = "")]
        suffix: String,
        /// Text removed from the start of the editor ID of the copy.
        #[arg(long, default_value = "")]
        prefix_remove: String,
        /// Text removed from the end of the editor ID of the copy.
        #[arg(long, default_value = "")]
        suffix_remove: String,
        /// Report the masters the copy needs and whether the target has the record, but copy nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove a record and its child group from a plugin (records.delete, xEdit's Remove). Needs --edit unless --dry-run.
    Delete {
        /// Load order FormID as hexadecimal digits.
        form_id: String,
        /// Plugin whose version of the record is removed; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the record, but remove nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Copy the records that refer to injected records of a plugin which is not their master into that plugin and remove those references from the originals (records.cleanup_injected, xEdit's "Cleanup injected records"). Needs --edit unless --dry-run.
    CleanupInjected {
        /// Load order FormIDs of the records; every record of the plugin that refers to such injected records when none is given.
        form_ids: Vec<String>,
        /// Plugin whose records are cleaned up; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the records and the masters, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum ElementsAction {
    /// Read an element of a record by its path, such as DATA\\Health.
    Get {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// Path of the element inside the record, with \\ between the names.
        path: String,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Levels of child elements to include; all when omitted.
        #[arg(long)]
        depth: Option<usize>,
    },
    /// Set the value of an element by its path (elements.set). Needs --edit unless --dry-run.
    Set {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// Path of the element inside the record, with \\ between the names. A missing last element is added.
        path: String,
        /// The edit value, as xEdit shows it in its editor. Omit with --default.
        value: Option<String>,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Set the value as a native value: a JSON number or boolean.
        #[arg(long)]
        native: bool,
        /// Set the element to the default of its definition.
        #[arg(long)]
        default: bool,
        /// Report the element and what would change, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Add a member, an array entry or a child record (elements.add, xEdit's Add). Needs --edit unless --dry-run.
    Add {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// What to add: a member name or signature (EDID, FULL), a child record signature of a cell, topic, worldspace or quest (REFR, INFO, CELL[3,-2], CELL[P]), or an array position.
        name: String,
        /// Path of the container inside the record to add to; the record itself when omitted.
        #[arg(long)]
        path: Option<String>,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the container, but add nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove an element of a record by its path (elements.remove, xEdit's Remove). Needs --edit unless --dry-run.
    Remove {
        /// Load order FormID of the record as hexadecimal digits.
        form_id: String,
        /// Path of the element inside the record, with \\ between the names.
        path: String,
        /// Plugin the record is seen from; the last loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the element, but remove nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum MastersAction {
    /// Add loaded plugins as masters (masters.add, AddMastersIfMissing), then sort the masters by load order. Needs --edit unless --dry-run.
    Add {
        /// File names of loaded plugins to add, such as Dawnguard.esm.
        #[arg(required = true)]
        masters: Vec<String>,
        /// Plugin to change; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Keep the masters in the order they are added instead of sorting them by load order.
        #[arg(long)]
        no_sort: bool,
        /// Report the master list the command would leave, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the masters no FormID of the plugin points to (masters.clean, CleanMasters). Needs --edit unless --dry-run.
    Clean {
        /// Plugin to change; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the master list the command would leave, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Sort the masters by load order (masters.sort, SortMasters). Needs --edit unless --dry-run.
    Sort {
        /// Plugin to change; the only loaded plugin when omitted.
        #[arg(long)]
        file: Option<String>,
        /// Report the master list the command would leave, but change nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

/// The command name and parameters of a subcommand.
fn command_of(action: Action) -> Result<(String, Value), CommandError> {
    Ok(match action {
        Action::Dump { .. }
        | Action::Saves { .. }
        | Action::Schema
        | Action::Batch { .. }
        | Action::Serve { .. }
        | Action::Mcp => {
            unreachable!("handled before")
        }
        Action::Session {
            action: SessionAction::Info,
        } => ("session.info".to_owned(), json!({})),
        Action::Files {
            action: FilesAction::List,
        } => ("files.list".to_owned(), json!({})),
        Action::Files {
            action:
                FilesAction::Flags {
                    file,
                    esm,
                    light,
                    medium,
                    update,
                    blueprint,
                    localized,
                    dry_run,
                },
        } => (
            "files.flags".to_owned(),
            json!({
                "file": file, "esm": esm, "light": light, "medium": medium, "update": update,
                "blueprint": blueprint, "localized": localized, "dry_run": dry_run
            }),
        ),
        Action::Refs { action } => match action {
            RefsAction::Get {
                form_id,
                file,
                offset,
                limit,
            } => (
                "refs.get".to_owned(),
                json!({ "form_id": form_id, "file": file, "offset": offset, "limit": limit }),
            ),
            RefsAction::Dump => unreachable!("handled before"),
            RefsAction::Build { file, only_load } => {
                ("refs.build".to_owned(), json!({ "file": file, "only_load": only_load }))
            }
        Action::Archive { action } => match action {
            ArchiveAction::List {
                archive,
                files,
                folder,
                offset,
                limit,
            } => (
                "archive.list".to_owned(),
                json!({ "archive": archive, "files": files, "folder": folder, "offset": offset, "limit": limit }),
            ),
            ArchiveAction::Extract {
                archive,
                output,
                threads,
                dry_run,
            } => (
                "archive.extract".to_owned(),
                json!({ "archive": archive, "output": output, "threads": threads, "dry_run": dry_run }),
            ),
            ArchiveAction::Pack {
                archive,
                sources,
                format,
                compress,
                split,
                filters,
                no_share,
                threads,
                archive_flags,
                file_flags,
                dry_run,
            } => (
                "archive.pack".to_owned(),
                json!({
                    "archive": archive, "sources": sources, "format": format, "compress": compress,
                    "split": split, "filters": filters, "share": !no_share, "threads": threads,
                    "archive_flags": archive_flags, "file_flags": file_flags, "dry_run": dry_run
                }),
            ),
        },
        Action::Formids { action } => match action {
            FormidsAction::Change {
                form_id,
                new_form_id,
                file,
                target_file,
                overrides,
                dry_run,
            } => (
                "formids.change".to_owned(),
                json!({
                    "form_id": form_id, "new_form_id": new_form_id, "file": file,
                    "target_file": target_file, "overrides": overrides, "dry_run": dry_run
                }),
            ),
            FormidsAction::Renumber {
                file,
                start,
                compact,
                inject_into,
                preserve_object_ids,
                all_or_nothing,
                dry_run,
            } => (
                "formids.renumber".to_owned(),
                json!({
                    "file": file, "start": start, "compact": compact, "inject_into": inject_into,
                    "preserve_object_ids": preserve_object_ids, "all_or_nothing": all_or_nothing, "dry_run": dry_run
                }),
            ),
        },
        Action::Records { action } => match action {
            RecordsAction::List {
                file,
                signature,
                offset,
                limit,
            } => (
                "records.list".to_owned(),
                json!({ "file": file, "signature": signature, "offset": offset, "limit": limit }),
            ),
            RecordsAction::Find {
                file,
                signature,
                editor_id,
                name,
                limit,
            } => (
                "records.find".to_owned(),
                json!({ "file": file, "signature": signature, "editor_id": editor_id, "name": name, "limit": limit }),
            ),
            RecordsAction::Get { form_id, file, depth } => (
                "records.get".to_owned(),
                json!({ "form_id": form_id, "file": file, "depth": depth }),
            ),
            RecordsAction::Copy {
                form_id,
                to,
                from,
                as_new,
                deep,
                prefix,
                suffix,
                prefix_remove,
                suffix_remove,
                dry_run,
            } => (
                "records.copy".to_owned(),
                json!({ "form_id": form_id, "to": to, "from": from, "as_new": as_new, "deep": deep,
                        "prefix": prefix, "suffix": suffix, "prefix_remove": prefix_remove,
                        "suffix_remove": suffix_remove, "dry_run": dry_run }),
            ),
            RecordsAction::Delete { form_id, file, dry_run } => (
                "records.delete".to_owned(),
                json!({ "form_id": form_id, "file": file, "dry_run": dry_run }),
            ),
            RecordsAction::CleanupInjected {
                form_ids,
                file,
                dry_run,
            } => (
                "records.cleanup_injected".to_owned(),
                json!({ "form_ids": form_ids, "file": file, "dry_run": dry_run }),
            ),
        },
        Action::Elements { action } => match action {
            ElementsAction::Get {
                form_id,
                path,
                file,
                depth,
            } => (
                "elements.get".to_owned(),
                json!({ "form_id": form_id, "path": path, "file": file, "depth": depth }),
            ),
            ElementsAction::Set {
                form_id,
                path,
                value,
                file,
                native,
                default,
                dry_run,
            } => {
                let value = match (default, value) {
                    (true, _) | (false, None) => Value::Null,
                    (false, Some(text)) if native => serde_json::from_str(&text).map_err(|e| {
                        CommandError::new(
                            "invalid_params",
                            format!("--native needs a JSON number or boolean: {e}"),
                        )
                    })?,
                    (false, Some(text)) => Value::String(text),
                };
                (
                    "elements.set".to_owned(),
                    json!({ "form_id": form_id, "path": path, "file": file, "value": value, "dry_run": dry_run }),
                )
            }
            ElementsAction::Add {
                form_id,
                name,
                path,
                file,
                dry_run,
            } => (
                "elements.add".to_owned(),
                json!({ "form_id": form_id, "name": name, "path": path, "file": file, "dry_run": dry_run }),
            ),
            ElementsAction::Remove {
                form_id,
                path,
                file,
                dry_run,
            } => (
                "elements.remove".to_owned(),
                json!({ "form_id": form_id, "path": path, "file": file, "dry_run": dry_run }),
            ),
        },
        Action::Masters { action } => match action {
            MastersAction::Add {
                masters,
                file,
                no_sort,
                dry_run,
            } => (
                "masters.add".to_owned(),
                json!({ "masters": masters, "file": file, "sort": !no_sort, "dry_run": dry_run }),
            ),
            MastersAction::Clean { file, dry_run } => {
                ("masters.clean".to_owned(), json!({ "file": file, "dry_run": dry_run }))
            }
            MastersAction::Sort { file, dry_run } => {
                ("masters.sort".to_owned(), json!({ "file": file, "dry_run": dry_run }))
            }
        },
        Action::Clean {
            file,
            itm,
            udr,
            quick,
            dry_run,
            output,
            no_backup,
        } => (
            "files.clean".to_owned(),
            json!({
                "file": file, "itm": itm, "udr": udr, "quick": quick, "dry_run": dry_run,
                "output": output, "backup": !no_backup
            }),
        ),
        Action::Save {
            file,
            output,
            dry_run,
            no_backup,
        } => (
            "files.save".to_owned(),
            json!({ "file": file, "output": output, "dry_run": dry_run, "backup": !no_backup }),
        ),
        Action::Conflicts {
            file,
            signature,
            min_conflict_all,
            conflict_this,
            include_single,
            master_and_leafs,
            quick_show_conflicts,
            offset,
            limit,
        } => (
            "conflicts.list".to_owned(),
            json!({
                "files": file, "signatures": signature, "min_conflict_all": min_conflict_all,
                "conflict_this": conflict_this, "include_single": include_single,
                "master_and_leafs": master_and_leafs, "quick_show_conflicts": quick_show_conflicts,
                "offset": offset, "limit": limit
            }),
        ),
        Action::Compare {
            form_id,
            file,
            master_and_leafs,
            hide_no_conflict,
            include_hidden,
        } => (
            "records.compare".to_owned(),
            json!({
                "form_id": form_id, "file": file, "master_and_leafs": master_and_leafs,
                "hide_no_conflict": hide_no_conflict, "include_hidden": include_hidden
            }),
        ),
        Action::Assets { action } => match action {
            AssetsAction::Dump {
                file,
                archive_path,
                kind,
                format,
                decimals,
                euler,
            } => (
                "assets.dump".to_owned(),
                json!({ "file": file, "archive_path": archive_path, "kind": kind, "format": format, "decimals": decimals, "euler": euler }),
            ),
            AssetsAction::Blocks {
                file,
                archive_path,
                kind,
            } => (
                "assets.blocks".to_owned(),
                json!({ "file": file, "archive_path": archive_path, "kind": kind }),
            ),
            AssetsAction::Types => ("assets.types".to_owned(), json!({})),
            AssetsAction::Save {
                file,
                output,
                archive_path,
                kind,
                dry_run,
            } => (
                "assets.save".to_owned(),
                json!({ "file": file, "output": output, "archive_path": archive_path, "kind": kind, "dry_run": dry_run }),
            ),
            AssetsAction::Set {
                file,
                path,
                value,
                output,
                block,
                archive_path,
                kind,
                dry_run,
            } => (
                "assets.set".to_owned(),
                json!({
                    "file": file,
                    "edits": [{ "block": block, "path": path, "value": value }],
                    "output": output,
                    "archive_path": archive_path,
                    "kind": kind,
                    "dry_run": dry_run,
                }),
            ),
            AssetsAction::FromJson {
                file,
                output,
                kind,
                dry_run,
            } => (
                "assets.from-json".to_owned(),
                json!({ "file": file, "output": output, "kind": kind, "dry_run": dry_run }),
            ),
        },
        Action::Sniff { action } => match action {
            SniffAction::List => ("sniff.list".to_owned(), json!({})),
            SniffAction::Run {
                operation,
                input,
                output,
                settings,
                set,
                path_contains,
                no_subdir,
                skip_on_errors,
                copy_all,
                threads,
                log,
                dry_run,
            } => {
                let mut options = serde_json::Map::new();
                for pair in set {
                    let (name, value) = pair.split_once('=').ok_or_else(|| {
                        CommandError::new("invalid_params", format!("--set {pair}: expected NAME=VALUE"))
                    })?;
                    options.insert(name.to_owned(), json!(value));
                }
                (
                    "sniff.run".to_owned(),
                    json!({
                        "operation": operation, "input": input, "output": output, "settings": settings,
                        "options": options, "path_contains": path_contains, "subdir": !no_subdir,
                        "skip_on_errors": skip_on_errors, "copy_all": copy_all, "threads": threads,
                        "log": log, "dry_run": dry_run
                    }),
                )
            }
        },
        Action::Lodgen {
            worldspaces,
            set,
            settings,
            output,
            scripts,
            temp,
            data,
            game_ini,
            seed,
            split_trees,
            dry_run,
        } => {
            let mut options = serde_json::Map::new();
            for pair in set {
                let (name, value) = pair
                    .split_once('=')
                    .ok_or_else(|| CommandError::new("invalid_params", format!("--set {pair}: expected NAME=VALUE")))?;
                options.insert(name.to_owned(), json!(value));
            }
            (
                "lodgen.generate".to_owned(),
                json!({
                    "worldspaces": worldspaces, "options": options, "settings": settings, "output": output,
                    "scripts": scripts, "temp": temp, "data": data, "game_ini": game_ini, "seed": seed, "split_trees": split_trees,
                    "dry_run": dry_run
                }),
            )
        }
        Action::Call { name, params } => {
            let params = serde_json::from_str(&params)
                .map_err(|e| CommandError::new("invalid_params", format!("--params is not valid JSON: {e}")))?;
            (name, params)
        }
    })
}

fn run(game: Option<String>, load: Vec<String>, edit: bool, action: Action) -> Result<Value, CommandError> {
    let registry = Registry::standard();
    if let Action::Schema = action {
        return Ok(registry.catalogue());
    }
    let batch = match &action {
        Action::Batch { file, keep_going } => {
            let text = if file == "-" {
                let mut text = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
                    .map_err(|e| CommandError::new("io", e.to_string()))?;
                text
            } else {
                std::fs::read_to_string(file).map_err(|e| CommandError::new("io", format!("{file}: {e}")))?
            };
            Some((parse_batch(&text)?, *keep_going))
        }
        _ => None,
    };
    let mut session = engine::open_session(game.as_deref(), &load, edit)?;
    if let Some((commands, keep_going)) = batch {
        // Every command runs in the one session, so an edit is visible to
        // the commands after it and a save at the end writes it.
        return Ok(registry.run_batch(&mut session, commands, keep_going));
    }
    let (name, params) = command_of(action)?;
    registry.call(&mut session, &name, params)
}

/// Runs a dump on a thread with a large stack, because the element tree
/// resolves deeply through the definitions. The progress messages go to
/// stderr like the log of xDump.
fn run_dump(dump: impl FnOnce(&mut dyn std::io::Write) -> Result<(), String> + Send + 'static) -> ExitCode {
    xedit_session::dump::log_progress_to_stderr();
    let worker = std::thread::Builder::new()
        .stack_size(xedit_core::threads::STACK_SIZE)
        .spawn(move || {
            let stdout = std::io::stdout();
            let mut out = std::io::BufWriter::with_capacity(1 << 20, stdout.lock());
            dump(&mut out)
        })
        .expect("the dump thread");
    let result = worker
        .join()
        .unwrap_or_else(|_| Err("the dump thread panicked".to_owned()));
    if let Err(error) = result {
        eprintln!("error: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `xedit serve`: the session stays loaded, the commands are JSON-RPC methods.
fn run_serve(
    game: Option<String>,
    load: Vec<String>,
    edit: bool,
    pipe: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let engine = engine::Engine::open(game.as_deref(), &load, edit)?;
    let mut handler = serve::ServeHandler::new(engine);
    match pipe {
        None => rpc::serve_lines(
            BufReader::new(std::io::stdin().lock()),
            std::io::stdout().lock(),
            &mut handler,
        )?,
        Some(name) => pipe::listen(&name, |file| {
            rpc::serve_lines(BufReader::new(&file), &file, &mut handler)?;
            Ok(!handler.shutdown_requested())
        })?,
    }
    Ok(())
}

/// `xedit mcp`: the registry as MCP tools on stdio.
fn run_mcp(game: Option<String>, load: Vec<String>, edit: bool) -> Result<(), Box<dyn std::error::Error>> {
    if game.is_none() && !load.is_empty() {
        return Err(CommandError::new("invalid_params", "--load needs --game").into());
    }
    let mut handler = mcp::McpHandler::new(engine::Engine::lazy(game, load, edit));
    rpc::serve_lines(
        BufReader::new(std::io::stdin().lock()),
        std::io::stdout().lock(),
        &mut handler,
    )?;
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    xedit_core::threads::set_threads(cli.threads.unwrap_or(0));
    xedit_session::refs::set_cache_options(
        cli.cache_path.as_deref(),
        cli.dont_cache,
        cli.dont_cache_load,
        cli.dont_cache_save,
    );
    // `-FillPNAM`, and the settings `-quickautoclean` gives the load.
    xedit_session::commands::set_fill_pnam_on_load(cli.fill_pnam);
    xedit_session::commands::set_quick_clean_on_load(matches!(cli.action, Action::Clean { quick: true, .. }));
    if matches!(cli.action, Action::Serve { .. } | Action::Mcp) {
        // stdout carries the protocol; the progress of a load goes to stderr.
        xedit_session::dump::log_progress_to_stderr();
        let outcome = match cli.action {
            Action::Serve { pipe } => run_serve(cli.game, cli.load, cli.edit, pipe),
            _ => run_mcp(cli.game, cli.load, cli.edit),
        };
        return match outcome {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if let Action::Dump { game, file } = cli.action {
        return run_dump(move |out| {
            let mode = xedit_session::dump::setup_game(&game)?;
            xedit_session::dump::dump_file(&file, mode, out)
        });
    }
    if let Action::Saves {
        action: SavesAction::Dump { game, data, file },
    } = cli.action
    {
        return run_dump(move |out| {
            let mode = xedit_session::dump::setup_saves(&game, &file)?;
            xedit_session::dump::dump_save(&file, &data, mode, out)
        });
    }
    // The progress messages of a load and a save go to stderr like the log
    // of xEdit; the result goes to stdout.
    xedit_session::dump::log_progress_to_stderr();
    if let Action::Refs {
        action: RefsAction::Dump,
    } = cli.action
    {
        let (game, load, edit) = (cli.game, cli.load, cli.edit);
        return run_dump(move |out| {
            let mut session = engine::open_session(game.as_deref(), &load, edit).map_err(|error| error.to_string())?;
            xedit_session::refs::write_index(&mut session, out).map_err(|error| error.to_string())
        });
    }
    let outcome = run(cli.game, cli.load, cli.edit, cli.action);
    match (&outcome, cli.json) {
        (Ok(result), true) => println!("{}", json!({ "ok": true, "result": result })),
        (Ok(result), false) => println!("{result:#}"),
        (Err(error), true) => println!("{}", json!({ "ok": false, "error": error })),
        (Err(error), false) => eprintln!("error: {error}"),
    }
    if outcome.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
